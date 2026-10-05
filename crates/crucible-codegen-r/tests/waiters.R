args <- commandArgs(trailingOnly = TRUE)
# Load lazily used namespaces up front so call deadlines measure calls, not package loading.
invisible(lapply(c('curl', 'openssl', 'jsonlite', 'stats'), loadNamespace))
suppressPackageStartupMessages(library(jobs.client, lib.loc = args[1L]))
ns <- asNamespace('jobs.client')
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)
rejects <- function(expression, class = 'error') { result <- tryCatch({ force(expression); NULL }, error = identity); if (is.null(result) || !inherits(result, class)) stop('Expected ', class, ', got ', paste(format(result), collapse = ' ')); invisible(result) }
response <- function(status = 200L, body = '{}', headers = list()) list(status = status, headers = headers, body = charToRaw(body))
fake <- function(handler) {
  state <- new.env(); state$requests <- list()
  state$send <- function(request, read_body, scope) { state$requests[[length(state$requests) + 1L]] <- request; handler(length(state$requests), request) }
  state
}
sdk <- function(state, ...) JobsClient(client_config(endpoint = 'https://example.test', transport = state$send, base_delay = 0, ...))

# Retry acceptor on a modeled error name, jsonName-aware queries, terminal success.
test <- fake(function(attempt, request) {
  if (attempt == 1L) return(response(404L, '{"__type":"Missing"}'))
  response(body = if (attempt >= 3L) '{"wire-status":"READY"}' else '{"wire-status":"PENDING"}')
})
output <- sdk(test)$wait_until_ready(Input(), timeout = 30, min_delay = 0, max_delay = 0)
check(output$status == 'READY' && length(test$requests) == 3L, 'Waiter retries until success')

# Projections, filters, input/output paths, and timestamps compared as epoch seconds.
payload <- '{"items":[{"wire-status":"READY"},{"wire-status":"READY"}],"updatedAt":"2026-01-01T00:00:00Z"}'
test <- fake(function(attempt, request) response(body = payload))
client <- sdk(test)
for (name in c('all_ready', 'any_ready', 'filtered_ready', 'updated')) client[[paste0('wait_until_', name)]](Input(), timeout = 30)
input <- Input(names = list('a', 'b'))
invisible(client$wait_until_counts_match(input, timeout = 30))
check(identical(input$names, list('a', 'b')), 'Waiter does not change caller input')
check(length(test$requests) == 5L, 'Each waiter matched on its first attempt')
test <- fake(function(attempt, request) response(body = '{"items":[{"wire-status":"READY"},{"wire-status":"FAILED"}],"updatedAt":"1969-12-31T23:59:59Z"}'))
client <- sdk(test)
rejects(client$wait_until_all_ready(Input(), timeout = .3, min_delay = 0, max_delay = 0), 'crucible_waiter_timeout')
check(length(test$requests) > 1L, 'Unmatched acceptors retry')
invisible(client$wait_until_any_ready(Input(), timeout = 30))
rejects(client$wait_until_updated(Input(), timeout = .2), 'crucible_waiter_timeout')

# A failure acceptor carries its modeled message.
test <- fake(function(attempt, request) response(body = '{"wire-status":"FAILED","reasons":["job rejected"]}'))
error <- rejects(sdk(test)$wait_until_ready(Input(), timeout = 30), 'crucible_waiter_error')
check(!inherits(error, 'crucible_waiter_timeout') && identical(error$reason, list('job rejected')) && error$attempts == 1L, 'Failure acceptor message')
check(grepl('job rejected', conditionMessage(error), fixed = TRUE), 'Failure message text')

# An unmatched error is terminal and keeps the service error.
test <- fake(function(attempt, request) response(403L, '{"__type":"Denied"}'))
error <- rejects(sdk(test)$wait_until_ready(Input(), timeout = 30), 'crucible_waiter_error')
check(inherits(error$error, 'crucible_service_error') && error$error$metadata$name == 'Denied', 'Terminal error waiter')

# The last delay is clamped to the deadline, so one attempt fits a short timeout.
test <- fake(function(attempt, request) response(body = '{"wire-status":"PENDING"}'))
started <- proc.time()[['elapsed']]
error <- rejects(sdk(test)$wait_until_ready(Input(), timeout = .2), 'crucible_waiter_timeout')
elapsed <- proc.time()[['elapsed']] - started
check(length(test$requests) == 1L && error$attempts == 1L && elapsed >= .15 && elapsed < 1.5, 'Waiter deadline')
rejects(sdk(test)$wait_until_ready(Input(), timeout = 0))
rejects(sdk(test)$wait_until_ready(Input(), timeout = 1, min_delay = 3, max_delay = 2))

# Exponential jitter between the modeled bounds.
delay <- get('.crucible_waiter_delay', ns)
upper <- function(low, high) high
check(identical(vapply(1:5, function(n) delay(2, 8, n, 100, upper), 0), c(2, 4, 8, 8, 8)), 'Waiter backoff ceiling')
check(delay(2, 8, 2, 5, upper) == 5, 'Waiter final delay reaches the deadline')
values <- vapply(1:50, function(n) delay(2, 8, 3, 100), 0)
check(all(values >= 2 & values <= 8) && length(unique(values)) > 1L, 'Waiter jitter range')

# Cancellation ends a wait between polls and is not reported as a waiter failure.
polls <- 0L
cancel <- cancellation_token(function() polls >= 2L)
test <- fake(function(attempt, request) { polls <<- polls + 1L; response(body = '{"wire-status":"PENDING"}') })
started <- proc.time()[['elapsed']]
rejects(sdk(test)$wait_until_ready(Input(), timeout = 30, min_delay = .05, max_delay = .05, cancellation = cancel), 'crucible_cancelled')
check(proc.time()[['elapsed']] - started < 2, 'Cancelled waiter returns promptly')

# Compiled JMESPath keeps exact numbers and distinguishes booleans.
query <- get('.crucible_query', ns)
literal <- function(value) list(kind = 'literal', value = value)
call <- function(name, ...) query(list(kind = 'function', `function` = name, arguments = list(...)), NULL)
check(identical(as.character(call('to_number', literal('12345678901234567890.0000000001'))), '12345678901234567890.0000000001'), 'Exact to_number')
check(identical(call('contains', literal(list(TRUE)), literal(json_number('1'))), FALSE), 'Boolean is not number')
sorted <- call('sort', literal(list(json_number('2.2'), json_number('1.1'), json_number('10'))))
check(identical(vapply(sorted, as.character, ''), c('1.1', '2.2', '10')), 'Numeric sort')
check(call('type', literal(json_number('1.1'))) == 'number', 'Number type')
check(call('to_string', literal(json_parse('{"number":1.000000000000000000001}'))) == '{"number":1.000000000000000000001}', 'Exact to_string')
check(as.character(call('sum', literal(json_parse('[0.1,0.2,10000000000000000000000]')))) == '10000000000000000000000.3', 'Exact sum')
check(as.character(call('avg', literal(json_parse('[1,2]')))) == '1.5', 'Average')
check(as.character(call('floor', literal(json_number('-1.5')))) == '-2' && as.character(call('ceil', literal(json_number('1.01')))) == '2', 'Floor and ceil')
check(call('length', literal('héllo')) == '5', 'Code point length')
compare <- function(op, a, b) query(list(kind = 'compare', comparator = op, left = literal(a), right = literal(b)), NULL)
check(isTRUE(compare('lessThan', json_number('1e2'), json_number('101'))) && isTRUE(compare('equal', json_number('1.0'), json_number('1'))), 'Exact comparisons')
check(is.null(compare('lessThan', 'a', 'b')) && identical(compare('equal', TRUE, json_number('1')), FALSE), 'Comparison types')
slice <- query(list(kind = 'slice', start = NULL, stop = NULL, step = json_number('-1')), list(1, 2, 3))
check(identical(slice, list(3, 2, 1)), 'Reverse slice')

if (nzchar(Sys.getenv('ECHO_URL'))) {
  # The same waiter over native HTTP against the loopback fixture.
  responses <- '[[200,"{\\"wire-status\\":\\"PENDING\\"}"],[200,"{\\"wire-status\\":\\"READY\\"}"]]'
  client <- JobsClient(client_config(endpoint = Sys.getenv('ECHO_URL'), headers = list('x-fixture-responses' = responses)))
  check(client$wait_until_ready(Input(), timeout = 10, min_delay = 0, max_delay = .05)$status == 'READY', 'Native HTTP waiter')
  client$close()
}
cat('R waiters and compiled queries passed\n')
