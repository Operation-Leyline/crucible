args <- commandArgs(trailingOnly = TRUE)
# Load lazily used namespaces up front so call deadlines measure calls, not package loading.
invisible(lapply(c('curl', 'openssl', 'jsonlite', 'stats'), loadNamespace))
suppressPackageStartupMessages(library(checksum.client, lib.loc = args[1L]))
ns <- asNamespace('checksum.client')
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)
rejects <- function(expression, class = 'error') { result <- tryCatch({ force(expression); NULL }, error = identity); if (is.null(result) || !inherits(result, class)) stop('Expected ', class, ', got ', paste(format(result), collapse = ' ')); invisible(result) }
vectors <- jsonlite::fromJSON(Sys.getenv('CHECKSUM_CASES'), simplifyVector = FALSE)
data <- charToRaw(vectors$inputUtf8)
digest <- get('.crucible_checksum', ns)
for (algorithm in names(vectors$base64)) check(identical(digest(algorithm, data), vectors$base64[[algorithm]]), paste('Checksum vector', algorithm))
check(identical(digest('MD5', raw()), vectors$emptyMd5), 'Empty MD5 vector')
check(identical(digest('CRC32', raw()), 'AAAAAA==') && identical(digest('CRC64NVME', raw()), 'AAAAAAAAAAA='), 'Empty CRC vectors')
rejects(digest('XXHASH64', data))

mode <- 'normal'; observed <- list(); verified <- list()
transport <- function(request, read_body, scope) {
  observed[[length(observed) + 1L]] <<- request
  check(read_body, 'Checksum validation reads the body')
  algorithm <- request$headers[['x-algorithm']]
  if (!mode %in% c('override', 'skip')) {
    name <- if (is.null(algorithm)) 'content-md5' else paste0('x-amz-checksum-', tolower(algorithm))
    expected <- if (length(request$body)) vectors$base64[[if (is.null(algorithm)) 'MD5' else algorithm]] else vectors$emptyMd5
    check(identical(request$headers[[name]], expected), paste('Request checksum', name))
  }
  headers <- list('x-amz-checksum-xxhash3' = 'unmodeled')
  if (mode != 'unmodeled') headers[[paste0('x-amz-checksum-', tolower(if (is.null(algorithm)) 'CRC32' else algorithm))]] <- if (mode == 'bad') 'AAAA' else vectors$base64[[if (is.null(algorithm)) 'CRC32' else algorithm]]
  if (mode == 'one') headers[['x-amz-checksum-sha256']] <- 'AAAA'
  list(status = 200L, headers = headers, body = data)
}
client <- ChecksumsClient(client_config(endpoint = 'https://example.test', transport = transport, checksum_validated = function(operation, algorithm) verified[[length(verified) + 1L]] <<- c(operation, algorithm)))
invisible(client$legacy(Input(data = data))); invisible(client$required(Input(data = data))); invisible(client$legacy(Input(data = raw())))
input <- Input(data = data)
check(identical(client$flexible(input)$data, data), 'Flexible checksum round trip')
check(inherits(input$algorithm, 'crucible_unset') && inherits(input$validation, 'crucible_unset'), 'Caller model is not mutated')
check(observed[[length(observed)]]$headers[['x-validation']] == 'ENABLED' && observed[[length(observed)]]$headers[['x-algorithm']] == 'CRC32', 'Default algorithm and validation mode')
for (algorithm in names(vectors$base64)) {
  invisible(client$flexible(Input(data = data, algorithm = Algorithm(algorithm))))
  check(identical(verified[[length(verified)]], c('Flexible', algorithm)), paste('Validated', algorithm))
}
mode <- 'bad'; rejects(client$flexible(input), 'crucible_checksum_error')
mode <- 'one'; invisible(client$flexible(input))
mode <- 'unmodeled'; count <- length(verified); invisible(client$flexible(input)); check(length(verified) == count, 'Unmodeled algorithm is not validated')
mode <- 'override'; invisible(client$flexible(Input(data = data, checksum = 'supplied')))
check(observed[[length(observed)]]$headers[['x-amz-checksum-crc32']] == 'supplied', 'Caller checksum header wins')
count <- length(observed); rejects(client$flexible(Input(data = data, algorithm = Algorithm('XXHASH64')))); check(length(observed) == count, 'Unsupported algorithm fails before sending')
client$close()
mode <- 'skip'
client <- ChecksumsClient(client_config(endpoint = 'https://example.test', transport = transport, request_checksum_calculation = 'WHEN_REQUIRED', response_checksum_validation = 'WHEN_REQUIRED'))
invisible(client$flexible(Input(data = data)))
last <- observed[[length(observed)]]$headers
check(is.null(last[['x-algorithm']]) && is.null(last[['x-validation']]) && is.null(last[['x-amz-checksum-crc32']]), 'WHEN_REQUIRED skips optional checksums')
client$close()
mode <- 'override'
client <- ChecksumsClient(client_config(endpoint = 'https://example.test', transport = transport, middleware = list(function(request) { request$headers[['content-md5']] <- 'supplied'; request })))
invisible(client$legacy(Input(data = data))); check(observed[[length(observed)]]$headers[['content-md5']] == 'supplied', 'Middleware checksum header wins')
client$close()
rejects(client_config(request_checksum_calculation = 'ALWAYS'))

if (nzchar(Sys.getenv('ECHO_URL'))) {
  # The loopback fixture echoes the body and computes CRC32 and SHA256 itself.
  client <- ChecksumsClient(client_config(endpoint = Sys.getenv('ECHO_URL'), checksum_validated = function(operation, algorithm) verified[[length(verified) + 1L]] <<- c(operation, algorithm)))
  check(identical(client$flexible(Input(data = data, algorithm = Algorithm('SHA256')))$data, data) && identical(verified[[length(verified)]], c('Flexible', 'SHA256')), 'Native HTTP checksum validation')
  sent <- jsonlite::fromJSON(rawToChar(curl::curl_fetch_memory(paste0(Sys.getenv('ECHO_URL'), '/__requests'))$content), simplifyVector = FALSE)
  check(sent[[length(sent)]]$headers[['x-amz-checksum-sha256']] == vectors$base64$SHA256, 'Native HTTP request checksum')
  client$close()
}
cat('R HTTP checksums passed\n')
