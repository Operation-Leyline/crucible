args <- commandArgs(trailingOnly = TRUE)
# Load lazily used namespaces up front so call deadlines measure calls, not package loading.
invisible(lapply(c('curl', 'openssl', 'jsonlite', 'stats'), loadNamespace))
package <- args[2L]; version <- if (length(args) >= 3L) args[3L] else ''
suppressPackageStartupMessages(library(package, lib.loc = args[1L], character.only = TRUE))
sdk <- asNamespace(package)
check <- function(value, label) if (!isTRUE(value)) stop(paste(package, label), call. = FALSE)
rejects <- function(expression, class = 'error') { result <- tryCatch({ force(expression); NULL }, error = identity); if (is.null(result) || !inherits(result, class)) stop('Expected ', class, ', got ', paste(format(result), collapse = ' ')); invisible(result) }
instant <- timestamp('-1', 750000000L)
seen <- list()
transport <- function(request, read_body, scope) {
  seen[[length(seen) + 1L]] <<- request
  check(request$method == 'POST', 'POST')
  body <- json_parse(rawToChar(request$body))
  check(as.character(body$when) == '-0.25' && body$iso == '1969-12-31T23:59:59.75Z', 'Document timestamp formats')
  check(!'nullable' %in% names(body) && 'document' %in% names(body) && is.null(body$document), 'Null members are omitted except documents')
  check(identical(body$floatValue, 'NaN') && identical(body$doubleValue, 'Infinity'), 'Nonfinite floats as strings')
  check(identical(body[[if (version == '') 'wire-name' else 'renamed']], 'hello'), 'Protocol member names')
  if (version != '') {
    check(endsWith(request$url, 'example.test/'), 'AWS JSON posts to the root path')
    check(request$headers[['x-amz-target']] == 'JsonService.Echo' && request$headers[['content-type']] == paste0('application/x-amz-json-', version), 'AWS JSON target and media type')
    check(as.character(body$date) == '-0.25' && as.character(body$dates[[1L]]) == '-0.25', 'AWS JSON body timestamps')
  } else {
    check(grepl('/echo?', request$url, fixed = TRUE) && grepl('time=1969-12-31T23%3A59%3A59.75Z', request$url, fixed = TRUE), 'REST JSON query timestamp')
    check(request$headers[['x-date']] == 'Wed, 31 Dec 1969 23:59:59 GMT', 'REST JSON header timestamp')
    check(!'date' %in% names(body), 'Bound members stay out of the body')
  }
  body$choice <- json_object(text = 'ok', `__type` = 'ignored'); body['nullable'] <- list(NULL)
  headers <- if (version == '') list('x-date' = 'Wed, 31 Dec 1969 23:59:59 GMT', 'x-dates' = '"Wed, 31 Dec 1969 23:59:59 GMT", "Thu, 01 Jan 1970 00:00:00 GMT"') else list()
  list(status = 200L, headers = headers, body = charToRaw(json_encode(body)))
}
client <- sdk$JsonServiceClient(client_config(endpoint = 'https://example.test', transport = transport))
output <- client$echo(sdk$ValuesInput(renamed = 'hello', when = instant, iso = instant, date = instant, dates = list(instant), query = instant, nullable = NULL, document = NULL, float_value = NaN, double_value = Inf))
check(inherits(output$choice, 'Choice') && output$choice$member == 'text' && output$choice$value == 'ok', 'Unknown union __type is ignored')
check(inherits(output$nullable, 'crucible_unset') && is.null(output$document), 'Null members decode as unset')
check(is.nan(output$float_value) && identical(output$double_value, Inf), 'Nonfinite floats decode')
check(identical(output$when, instant) && identical(output$iso, instant), 'Timestamps round trip')
if (version == '') check(identical(output$date, instant) && length(output$dates) == 2L && identical(output$dates[[2L]], timestamp('0')), 'REST JSON header timestamps decode')
check(length(seen) == 1L, 'One request')
client$close()

for (discriminator in list(list(code = 'Problem'), list(`__type` = 'example.json#Problem:http://internal'))) {
  body <- json_encode(do.call(json_object, c(discriminator, list(Message = 'bad'))))
  client <- sdk$JsonServiceClient(client_config(endpoint = 'https://example.test', transport = function(...) list(status = 409L, headers = list('x-amzn-requestid' = 'request-42'), body = charToRaw(body))))
  error <- rejects(client$echo(sdk$ValuesInput()), 'ProblemFault')
  check(error$metadata$name == 'Problem' && error$metadata$request_id == 'request-42' && error$metadata$message == 'bad', 'AWS error discriminator')
  client$close()
}
client <- sdk$JsonServiceClient(client_config(endpoint = 'https://example.test', transport = function(...) list(status = 409L, headers = list('x-amzn-errortype' = 'Problem:http://internal/'), body = charToRaw('{"message":"header"}'))))
check(rejects(client$echo(sdk$ValuesInput()), 'ProblemFault')$metadata$message == 'header', 'AWS error header')
client$close()
client <- sdk$JsonServiceClient(client_config(endpoint = 'https://example.test', transport = function(request, ...) {
  check(identical(request$body, if (version == '') NULL else charToRaw('{}')), 'Empty operation body')
  if (version != '') check(request$headers[['x-amz-target']] == 'JsonService.Empty', 'Empty operation target')
  list(status = 200L, headers = list(), body = raw())
}))
check(is.null(client$empty()), 'Empty operation')
client$close()

if (nzchar(Sys.getenv('ECHO_URL'))) {
  # The loopback fixture echoes the request body, which every protocol decodes.
  client <- sdk$JsonServiceClient(client_config(endpoint = Sys.getenv('ECHO_URL')))
  output <- client$echo(sdk$ValuesInput(renamed = 'native', when = instant, float_value = -Inf))
  check(output$renamed == 'native' && identical(output$when, instant) && identical(output$float_value, -Inf), 'Native HTTP protocol round trip')
  client$close()
}
cat(package, 'JSON protocol qualification passed\n')
