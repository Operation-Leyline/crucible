library(wire.client, lib.loc = commandArgs(TRUE)[1L])
check <- function(value) if (!isTRUE(value)) stop(paste('Wire check failed:', deparse(substitute(value))), call. = FALSE)
failure <- FALSE
transport <- function(request, read_body, scope) {
  check(read_body)
  if (endsWith(request$url, '/document')) {
    check(request$headers[['content-type']] == 'application/json' && json_parse(rawToChar(request$body))$name == 'record')
    return(list(status = 200L, headers = list(), body = request$body))
  }
  check(request$headers[['content-type']] == 'text/plain' && rawToChar(request$body) == 'héllo')
  check(request$headers[['x-meta']] == 'eyJhIjoxfQ==, aGVsbG8=' && request$headers[['x-extra-trace']] == 'aGVsbG8=')
  if (failure) list(status = 422L, headers = list('x-crucible-error' = 'TextError'), body = charToRaw('invalid text')) else
    list(status = 200L, headers = list('x-meta' = 'eyJhIjoxfQ==, aGVsbG8=', 'x-extra-trace' = 'aGVsbG8='), body = request$body)
}
input <- TextInput(text = 'héllo', meta = list('{"a":1}', 'hello'), extras = list(trace = 'hello'))
client <- WireClient(client_config(endpoint = 'http://127.0.0.1', transport = transport))
output <- client$text(input)
check(identical(output$text, input$text) && identical(output$meta, input$meta) && identical(unclass(output$extras), input$extras))
check(client$document(DocumentInput(document = Record(name = 'record')))$document$name == 'record')
failure <- TRUE
error <- tryCatch(client$text(input), error = identity)
check(inherits(error, 'TextError') && error$model$details == 'invalid text' && error$metadata$status == 422)
client$close()
cat('R payloads and media headers passed\n')
