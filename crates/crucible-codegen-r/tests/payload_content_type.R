# Payload Content-Type precedence: a modelled Content-Type member, then the
# payload's media type, then a client-wide default header. test-r.py runs this
# once per protocol: arguments are the library path and the package name.
args <- commandArgs(TRUE)
library(args[2L], lib.loc = args[1L], character.only = TRUE)
check <- function(value, message) if (!isTRUE(value)) stop(paste('Payload Content-Type check failed:', message), call. = FALSE)
for (member in list(NULL, 'image/png')) for (configured in c(FALSE, TRUE)) {
  expected <- if (is.null(member)) 'application/octet-stream' else member
  transport <- function(request, read_body, scope) {
    check(identical(rawToChar(request$body), 'PNG'), 'body')
    check(sum(tolower(names(request$headers)) == 'content-type') == 1L, 'exactly one Content-Type header')
    check(request$headers[['content-type']] == expected, paste(request$headers[['content-type']], '!=', expected))
    list(status = 200L, headers = list(), body = raw())
  }
  headers <- if (configured) list('Content-Type' = 'image/jpeg') else list()
  client <- PayloadBindingsClient(client_config(endpoint = 'http://127.0.0.1', max_attempts = 1L, transport = transport, headers = headers))
  input <- if (is.null(member)) UploadInput(data = charToRaw('PNG')) else UploadInput(data = charToRaw('PNG'), content_type = member)
  client$upload(input)
  client$close()
}
cat(sprintf('R payload Content-Type precedence passed (%s)\n', args[2L]))
