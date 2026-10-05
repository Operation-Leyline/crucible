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
check(delay(2, 8, 2, 5, upper) == 3, 'Waiter final delay reserves a call budget')
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
for (case in list(
  list('[-0.01]', '-0.01'),
  list('[-0.01,0]', '-0.01'),
  list('[0,-0.01]', '-0.01'),
  list('[-0.01,0.02]', '0.01'),
  list('[0.01,-0.02]', '-0.01'),
  list('[-1.25,0.005]', '-1.245'),
  list('[0.005,-1.25]', '-1.245'),
  list('[10000000000000000000000.01,-10000000000000000000000]', '0.01'),
  list('[-10000000000000000000000,10000000000000000000000.01]', '0.01'),
  list('[1e-40,-2e-40]', paste0('-0.', strrep('0', 39L), '1'))
)) {
  check(identical(as.character(call('sum', literal(json_parse(case[[1L]])))), case[[2L]]),
        paste('Exact negative and mixed-sign sum', case[[1L]]))
}
check(as.character(call('avg', literal(json_parse('[1,2]')))) == '1.5', 'Average')
for (name in c('sum', 'avg')) {
  for (value in c('1e100001', '1e-100001', '1e1000000')) {
    error <- rejects(call(name, literal(list(json_number(value), json_number('1')))), 'crucible_query_error')
    check(conditionMessage(error) == 'JMESPath arithmetic exceeds supported precision', 'Alignment precision failure')
  }
}
# Copy the arithmetic closure to observe padding without changing the installed namespace.
padding <- new.env(parent = ns)
padding$calls <- 0L
padding$strrep <- function(...) { padding$calls <- padding$calls + 1L; stop('Padded before checking precision') }
guarded_add <- get('.crucible_decimal_add', ns)
environment(guarded_add) <- padding
rejects(guarded_add(list(negative = FALSE, digits = '1', exponent = 100001),
                    list(negative = FALSE, digits = '1', exponent = 0)), 'crucible_query_error')
check(padding$calls == 0L, 'Alignment rejected before padding allocation')
boundary <- as.character(call('sum', literal(list(json_number('1e100000'), json_number('1')))))
check(identical(boundary, paste0('1', strrep('0', 99999L), '1')), 'Exact alignment limit remains supported')
check(as.character(call('avg', literal(json_parse('[1,0,0]')))) == paste0('0.', strrep('3', 34L)), 'Average repeating remainder precision')
check(as.character(call('avg', literal(json_parse('[-1,0,0]')))) == paste0('-0.', strrep('3', 34L)), 'Negative average remainder precision')
check(as.character(call('avg', literal(json_parse('[0.01,0.01,0.02]')))) == paste0('0.01', strrep('3', 32L)), 'Small average uses 34 fractional digits')
check(as.character(call('avg', literal(json_parse('[-0.01,-0.01,-0.02]')))) == paste0('-0.01', strrep('3', 32L)), 'Negative small average uses 34 fractional digits')
check(as.character(call('avg', literal(json_parse('[500,0,0]')))) == paste0('166.', strrep('6', 34L)), 'Normalized coefficient retains 34 fractional digits')
check(as.character(call('avg', literal(json_parse('[-500,0,0]')))) == paste0('-166.', strrep('6', 34L)), 'Negative normalized coefficient retains 34 fractional digits')
check(as.character(call('avg', literal(json_parse('[1e-40,2e-40]')))) == paste0('0.', strrep('0', 39L), '1'), 'Tiny average retains the deeper exponent without extra padding')
precise_average <- '0.1234567890123456789012345678901234567890123456789'
check(as.character(call('avg', literal(list(json_number(precise_average))))) == precise_average, 'Single high-precision input remains exact')
check(is.null(call('avg', literal(list()))) && as.character(call('avg', literal(json_parse('[1,-1]')))) == '0', 'Empty and cancelling averages')
large_number <- paste0(strrep('7', 1000L), '2')
large_average <- as.character(call('avg', literal(list(json_number(large_number), json_number(large_number)))))
check(identical(large_average, large_number), 'Large coefficient average preserves exact digits')
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

check(delay(2, 8, 1, 1, upper) == 1, 'Short waiter budget is exhausted without another poll')

test <- fake(function(attempt, request) {
  if (attempt == 2L) Sys.sleep(.1)
  response(body = if (attempt == 1L) '{"wire-status":"PENDING"}' else '{"wire-status":"READY"}')
})
check(sdk(test)$wait_until_ready(Input(), timeout = 1.5, min_delay = 1, max_delay = 1)$status == 'READY' && length(test$requests) == 2L, 'Final poll retains a call budget')
