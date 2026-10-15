#' HTTP client settings
#' @description Total call deadlines cover retries and response reads. The default transport uses curl; a custom function receives request, read_body, and scope.
#' @param endpoint HTTP endpoint override; absent uses the generated endpoint.
#' @param token Bearer token provider invoked for each authenticated attempt.
#' @param transport Custom HTTP transport; callbacks must respect the supplied deadline and cancellation scope.
#' @param timeout Total call budget in seconds, including credentials, retries, backoff, and response reads.
#' @param max_attempts Maximum attempts including the first request (default 3).
#' @param base_delay Initial full-jitter exponential backoff bound in seconds.
#' @param max_delay Maximum backoff bound in seconds; Retry-After can override it.
#' @param headers Default request headers applied to every attempt.
#' @param middleware Request transformations applied on every attempt after authentication.
#' @return A validated settings list consumed by the service client constructor.
client_config <- function(endpoint = NULL, token = NULL, transport = NULL, timeout = 30, max_attempts = 3L, base_delay = .1, max_delay = 10, headers = list(), middleware = list()) {
  for (duration in list(timeout, base_delay, max_delay)) if (!is.numeric(duration) || length(duration) != 1L || !is.finite(duration) || duration < 0) stop('Invalid duration', call. = FALSE)
  if (timeout <= 0 || !is.numeric(max_attempts) || length(max_attempts) != 1L || !is.finite(max_attempts) || max_attempts < 1 || max_attempts != trunc(max_attempts)) stop('Invalid call limits', call. = FALSE)
  if (!is.null(token) && !is.function(token)) stop('Token provider must be a function', call. = FALSE)
  if (!is.null(transport) && !is.function(transport)) stop('Transport must be a function(request, read_body, scope)', call. = FALSE)
  if (!is.list(middleware) || any(!vapply(middleware, is.function, TRUE))) stop('Middleware must be a list of functions', call. = FALSE)
  list(endpoint = endpoint, token = token, transport = transport, timeout = timeout, max_attempts = max_attempts, base_delay = base_delay, max_delay = max_delay, headers = .crucible_headers(headers), middleware = middleware)
}
.crucible_headers <- function(headers) {
  if (!is.list(headers) || (length(headers) && is.null(names(headers)))) stop('Headers must be a named list', call. = FALSE)
  output <- list()
  for (name in names(headers)) {
    value <- .crucible_string(headers[[name]])
    if (!grepl("^[!#$%&'*+.^_`|~0-9A-Za-z-]+$", name) || grepl('[\r\n]', value)) stop('Invalid HTTP header', call. = FALSE)
    output[tolower(name)] <- list(value)
  }
  output
}
.crucible_url <- function(value) {
  value <- .crucible_string(value)
  if (grepl('[[:cntrl:] ]', value) || !grepl('^https?://[^/?#]+(?:[/?#]|$)', value, perl = TRUE)) stop('Absolute HTTP(S) URL required', call. = FALSE)
  authority <- sub('^https?://([^/?#]+).*$', '\\1', value)
  if (grepl('@', authority, fixed = TRUE)) stop('URL credentials are not allowed', call. = FALSE)
  authority
}
.crucible_request <- function(request) {
  if (!is.list(request)) stop('Middleware must return a request list', call. = FALSE)
  .crucible_url(request$url)
  if (!is.character(request$method) || length(request$method) != 1L || !grepl("^[!#$%&'*+.^_`|~0-9A-Za-z-]+$", request$method)) stop('Invalid HTTP method', call. = FALSE)
  if (!is.null(request$body) && !is.raw(request$body)) stop('Request body must be raw or NULL', call. = FALSE)
  if (identical(request$method, 'HEAD') && length(request$body)) stop('HEAD requests cannot have a body', call. = FALSE)
  request$headers <- .crucible_headers(request$headers)
  request
}
.crucible_transport <- function() {
  pool <- curl::new_pool()
  send <- function(request, read_body, scope) {
    handle <- curl::new_handle(); result <- NULL; failure <- NULL; status <- 0L; headers <- list()
    # The multi pool owns reusable connections; fresh easy handles prevent cookies
    # or per-request options from leaking between requests.
    curl::handle_setopt(handle, url = request$url, customrequest = request$method, nobody = identical(request$method, 'HEAD'), followlocation = FALSE,
      timeout_ms = min(2147483647, max(1, ceiling(scope$remaining() * 1000) + 25)), connecttimeout_ms = min(2147483647, max(1, ceiling(scope$remaining() * 1000) + 25)),
      suppress_connect_headers = TRUE, cookiefile = NULL, netrc = 0L, accept_encoding = 'identity', path_as_is = TRUE)
    wire_headers <- vapply(names(request$headers), function(name) if (request$headers[[name]] == '') paste0(name, ';') else paste0(name, ': ', request$headers[[name]]), '')
    curl::handle_setopt(handle, httpheader = c(wire_headers, 'Expect:', 'Accept-Encoding: identity'))
    if (!is.null(request$body)) curl::handle_setopt(handle, postfields = request$body)
    curl::multi_add(handle, done = function(response) result <<- response, fail = function(message) failure <<- message, pool = pool)
    on.exit(curl::multi_cancel(handle), add = TRUE)
    repeat {
      scope$check()
      # Older supported curl packages round a positive multi timeout to seconds.
      # Nonblocking progress plus an R sleep preserves subsecond deadlines.
      curl::multi_run(timeout = 0, pool = pool)
      scope$check()
      info <- curl::handle_data(handle)
      text <- rawToChar(info$headers)
      complete <- endsWith(text, '\r\n\r\n')
      if (complete) {
        headers <- list()
        for (line in strsplit(text, '\r\n', fixed = TRUE)[[1L]]) {
          if (grepl('^HTTP/[0-9.]+ [0-9]{3}', line)) { status <- as.integer(sub('^HTTP/[0-9.]+ ([0-9]{3}).*$', '\\1', line)); headers <- list() }
          else if (grepl(':', line, fixed = TRUE)) {
            name <- tolower(sub(':.*$', '', line)); value <- trimws(sub('^[^:]*:', '', line))
            headers[[name]] <- if (is.null(headers[[name]])) value else paste(headers[[name]], value, sep = ', ')
          }
        }
        if (status >= 200 && status < 300 && !read_body) return(list(status = status, headers = headers, body = raw()))
      }
      if (!is.null(failure)) stop(.crucible_condition('crucible_network', failure))
      if (!is.null(result)) {
        length_header <- headers[['content-length']]
        if (!is.null(length_header) && !grepl('^[0-9]+$', length_header)) stop(.crucible_condition('crucible_network', 'Malformed Content-Length'))
        return(list(status = result$status_code, headers = headers, body = result$content))
      }
      Sys.sleep(min(.01, scope$remaining()))
    }
  }
  attr(send, 'close') <- function() { for (handle in curl::multi_list(pool)) curl::multi_cancel(handle); pool <<- NULL; invisible(NULL) }
  send
}
.crucible_uuid <- function() {
  bytes <- as.integer(openssl::rand_bytes(16)); bytes[7L] <- bitwOr(bitwAnd(bytes[7L], 15L), 64L); bytes[9L] <- bitwOr(bitwAnd(bytes[9L], 63L), 128L)
  text <- paste0(sprintf('%02x', bytes), collapse = '')
  paste(substr(text,1,8),substr(text,9,12),substr(text,13,16),substr(text,17,20),substr(text,21,32),sep='-')
}
.crucible_prepare <- function(op, input, config, endpoint) {
  values <- input
  if (!is.null(op$token) && is.null(values[[op$token]])) values[[op$token]] <- .crucible_uuid()
  bindings <- op$request; path <- op$uri
  # Bound timestamps may use a different format than the member's document format.
  bound <- function(member) {
    value <- values[[member]]; format <- bindings[['timestampFormats']][[member]]
    if (is.null(value) || is.null(format)) value else .crucible_reformat(value, .crucible_shape(op$input)$members[[member]], format, TRUE)
  }
  for (name in bindings$labels) {
    value <- .crucible_bound(bound(name))
    if (value == '') stop('Empty HTTP label: ', name, call. = FALSE)
    greedy <- name %in% unlist(op$greedyLabels)
    # Appending a sentinel preserves trailing empty segments in base strsplit.
    parts <- if (greedy) head(strsplit(paste0(value, '/sentinel'), '/', fixed = TRUE)[[1L]], -1L) else value
    if (any(parts %in% c('.', '..'))) stop('Dot-segment HTTP label: ', name, call. = FALSE)
    path <- gsub(paste0('{', name, if (greedy) '+' else '', '}'), paste(vapply(parts, curl::curl_escape, character(1L)), collapse = '/'), path, fixed = TRUE)
  }
  query <- character()
  for (binding in bindings$query) {
    value <- bound(binding[[1L]])
    if (is.null(value)) next
    if (!is.list(value)) value <- list(value)
    for (item in value) query <- c(query, paste0(curl::curl_escape(binding[[2L]]), '=', curl::curl_escape(.crucible_bound(item))))
  }
  if (length(query)) path <- paste0(path, if (grepl('?', path, fixed = TRUE)) '&' else '?', paste(query, collapse = '&'))
  headers <- config$headers
  # A Content-Type bound to an input member outranks the payload's media type,
  # which outranks the client's default headers.
  modelled_content_type <- FALSE
  for (binding in bindings$headers) if (!is.null(values[[binding[[1L]]]])) {
    headers[[tolower(binding[[2L]])]] <- .crucible_bound(.crucible_media(bound(binding[[1L]]), bindings, binding[[1L]], TRUE))
    if (identical(tolower(binding[[2L]]), 'content-type')) modelled_content_type <- TRUE
  }
  body <- NULL
  if (length(bindings$body)) {
    object <- json_object()
    for (name in bindings$body) if (name %in% names(values)) object[name] <- values[name]
    body <- charToRaw(enc2utf8(json_encode(.crucible_document(object, op$input, TRUE)))); headers[['content-type']] <- 'application/json'
  }
  payload <- bindings$payload
  if (!is.null(payload) && !is.null(values[[payload[[1L]]]])) {
    value <- values[[payload[[1L]]]]; kind <- bindings$payloadKind
    body <- if (identical(kind, 'string')) charToRaw(enc2utf8(.crucible_string(value))) else if (identical(kind, 'document')) charToRaw(enc2utf8(json_encode(.crucible_document(value, .crucible_shape(op$input)$members[[payload[[1L]]]]$target, TRUE)))) else openssl::base64_decode(value)
    if (!modelled_content_type) headers[['content-type']] <- payload[[2L]]
  }
  # The release this client was generated for (docs/api-versions-clients.md).
  api_version <- .crucible_schema[['apiVersion']]
  if (is.character(api_version) && length(api_version) == 1L) headers[['crucible-version']] <- api_version
  list(request = .crucible_request(list(url = paste0(endpoint, path), method = op$method, headers = headers, body = body)))
}
.crucible_reformat <- function(value, description, format, encode) {
  shape <- .crucible_shape(description$target)
  if (shape$kind == 'list') return(lapply(value, function(item) if (is.null(item)) NULL else .crucible_reformat(item, shape$member, format, encode)))
  if (shape$kind != 'timestamp') return(value)
  document <- if (is.null(description$format)) shape$format else description$format
  if (encode) .crucible_time_encode(.crucible_time_decode(value, document), format) else .crucible_time_encode(.crucible_time_decode(value, format), document)
}
.crucible_apply_headers <- function(target, bindings, response, body) {
  for (binding in bindings$headers) {
    value <- response$headers[[tolower(binding[[2L]])]]
    if (!is.null(value)) {
      description <- .crucible_shape(target)$members[[binding[[1L]]]]; format <- bindings[['timestampFormats']][[binding[[1L]]]]
      parsed <- .crucible_header(description, value, format)
      if (!is.null(format)) parsed <- .crucible_reformat(parsed, description, format, FALSE)
      body[binding[[1L]]] <- list(.crucible_media(parsed, bindings, binding[[1L]], FALSE))
    }
  }
  body
}
.crucible_success <- function(op, response) {
  payload <- op$response$payload
  if (!is.null(payload)) { body <- json_object(); body[payload[[1L]]] <- list(.crucible_payload(op$output, op$response, response$body)) }
  else if (!length(op$response$body) || !length(response$body)) body <- json_object()
  else { body <- .crucible_document(json_parse(response$body), op$output); if (!.crucible_is_object(body)) stop('Expected response object', call. = FALSE) }
  if (!is.null(op$output)) body <- .crucible_apply_headers(op$output, op$response, response, body)
  body
}
.crucible_failure <- function(op, response) {
  body <- tryCatch(json_parse(response$body), error = function(e) json_object())
  if (!.crucible_is_object(body)) body <- json_object()
  text <- function(value) if (is.character(value) && length(value) == 1L && !inherits(value, c('crucible_number', 'crucible_long'))) value else NULL
  name <- text(body[['__type']])
  if (!is.null(response$headers[['x-crucible-error']])) name <- response$headers[['x-crucible-error']]
  matches <- Filter(function(target) { shape <- .crucible_shape(target); if (!is.null(name)) (if (!is.null(shape$error$wireCode)) shape$error$wireCode else shape$name) == name else as.numeric(shape$error$status) == response$status }, op$errors)
  target <- if (length(matches) == 1L) matches[[1L]] else NULL
  shape <- if (!is.null(target)) .crucible_shape(target) else NULL; traits <- shape$error
  if (is.null(name)) name <- if (!is.null(shape)) shape$name else 'UnknownError'
  message <- text(body[['message']])
  if (is.null(message)) message <- paste('HTTP', response$status)
  request_id <- response$headers[['x-request-id']]
  if (is.null(request_id)) request_id <- text(body[['requestId']])
  metadata <- list(name = name, message = message, status = response$status, request_id = request_id, headers = response$headers, retryable = isTRUE(traits$retryable), throttling = isTRUE(traits$throttling))
  model <- if (is.null(target)) NULL else tryCatch({
    object <- if (is.null(traits$payload)) .crucible_document(body, target) else { value <- json_object(); value[traits$payload[[1L]]] <- list(.crucible_payload(target, traits, response$body)); value }
    .crucible_codec(.crucible_apply_headers(target, traits, response, object), target)
  }, error = function(e) NULL)
  error <- .crucible_condition(c(if (!is.null(model)) shape$name, 'ServiceException', 'crucible_service_error'), message, metadata = metadata, model = model)
  error
}
.crucible_media <- function(value, bindings, member, encode) {
  if (!identical(bindings$headerEncodings[[member]], 'base64')) return(value)
  if (is.list(value)) return(lapply(value, .crucible_media, bindings = bindings, member = member, encode = encode))
  text <- .crucible_string(value)
  if (encode) return(as.character(openssl::base64_encode(charToRaw(enc2utf8(text)))))
  bytes <- openssl::base64_decode(text)
  if (!identical(as.character(openssl::base64_encode(bytes)), text)) stop('Invalid media-type header base64', call. = FALSE)
  .crucible_string(rawToChar(bytes))
}
.crucible_payload <- function(target, bindings, bytes) {
  if (identical(bindings$payloadKind, 'string')) return(.crucible_string(rawToChar(bytes)))
  if (identical(bindings$payloadKind, 'document')) {
    if (!length(bytes)) return(NULL)
    return(.crucible_document(json_parse(bytes), .crucible_shape(target)$members[[bindings$payload[[1L]]]]$target))
  }
  as.character(openssl::base64_encode(bytes))
}
.crucible_retry_delay <- function(config, attempt, header) {
  if (!is.null(header)) {
    numeric <- suppressWarnings(as.numeric(header))
    if (length(numeric) == 1L && is.finite(numeric) && numeric >= 0) return(numeric)
    date <- tryCatch(.crucible_time_decode(header, 'http-date'), error = function(e) NULL)
    if (!is.null(date)) return(max(0, as.numeric(date$seconds) - as.numeric(Sys.time())))
  }
  min(config$max_delay, config$base_delay * 2^min(attempt - 1, 30)) * stats::runif(1)
}
# Bearer is the one authentication scheme: an Authorization header the caller
# already set bypasses the token provider.
.crucible_has_bearer <- function(request) {
  value <- request$headers[['authorization']]
  !is.null(value) && startsWith(tolower(value), 'bearer ') && nchar(value) > 7L
}
.crucible_client <- function(config, endpoint) {
  if (!is.null(config$endpoint)) endpoint <- config$endpoint
  if (is.null(endpoint)) stop('An endpoint is required', call. = FALSE)
  .crucible_url(endpoint)
  if (grepl('[?#]', endpoint)) stop('Endpoint cannot contain query or fragment', call. = FALSE)
  endpoint <- sub('/+$', '', endpoint)
  transport <- if (is.null(config$transport)) .crucible_transport() else config$transport
  closed <- FALSE; client <- new.env(parent = emptyenv())
  client$close <- function() { if (!closed && is.null(config$transport)) attr(transport, 'close')(); closed <<- TRUE; transport <<- NULL; invisible(NULL) }
  operation <- function(name) { op <- .crucible_schema$operations[[name]]; if (is.null(op)) stop('Unknown operation: ', name, call. = FALSE); op }
  prepare <- function(op, values) .crucible_prepare(op, values, config, endpoint)
  send <- function(op, prepared, scope) {
    if (closed) stop('Client is closed', call. = FALSE)
    read_body <- length(op$response$body) > 0L || !is.null(op$response$payload)
    for (attempt in seq_len(config$max_attempts)) {
      scope$check(); request <- prepared$request
      # The token is resolved on every attempt so a refreshed token applies.
      token <- NULL; preset <- FALSE
      if (isTRUE(op$bearer)) {
        preset <- .crucible_has_bearer(request)
        if (!preset && !is.null(config$token)) { token <- config$token(); scope$check() }
      }
      for (middleware in config$middleware) { scope$check(); request <- .crucible_request(middleware(request)) }
      scope$check()
      if (!is.null(token)) request$headers[['authorization']] <- paste0('Bearer ', .crucible_string(token))
      request <- .crucible_request(request)
      authority <- tolower(.crucible_url(request$url)); hostname <- sub(':[0-9]+$', '', authority)
      credentialed <- preset || !is.null(token) || !is.null(request$headers[['authorization']])
      if (credentialed && !startsWith(tolower(request$url), 'https://') && !hostname %in% c('localhost','127.0.0.1','[::1]')) stop('Credentials require HTTPS or loopback HTTP', call. = FALSE)
      response <- tryCatch(transport(request, read_body, scope), crucible_network = function(e) e)
      scope$check()
      if (inherits(response, 'crucible_network')) {
        if (!isTRUE(op$safe) || attempt >= config$max_attempts) stop(response)
        scope$wait(.crucible_retry_delay(config, attempt, NULL)); next
      }
      response$headers <- .crucible_headers(response$headers)
      if (response$status >= 200 && response$status < 300) {
        return(.crucible_success(op, response))
      }
      error <- .crucible_failure(op, response)
      retry <- error$metadata$throttling || (isTRUE(op$safe) && (error$metadata$retryable || response$status %in% c(500,502,503,504)))
      if (!retry || attempt >= config$max_attempts) stop(error)
      scope$wait(.crucible_retry_delay(config, attempt, response$headers[['retry-after']]))
    }
    stop('No request attempt', call. = FALSE)
  }
  invoke <- function(name, input, timeout, cancellation) {
    scope <- .crucible_scope(timeout, cancellation); scope$check(); op <- operation(name)
    values <- if (is.null(op$input)) json_object() else .crucible_codec(input, op$input, TRUE)
    result <- send(op, prepare(op, values), scope)
    output <- if (is.null(op$output)) NULL else .crucible_codec(result, op$output)
    scope$check(); output
  }
  client$call <- function(name, input = NULL, cancellation = NULL) invoke(name, input, config$timeout, cancellation)
  client$pages <- function(name, input, cancellation = NULL) {
    op <- operation(name); pagination <- op$pagination
    if (is.null(pagination)) stop('Operation is not paginated', call. = FALSE)
    values <- .crucible_codec(input, op$input, TRUE); initial <- values[[pagination$input]]
    seen <- if (is.null(initial) || initial == '') character() else .crucible_string(initial)
    finished <- FALSE; cycle <- FALSE
    function() {
      if (cycle) stop('Pagination token cycle', call. = FALSE)
      if (finished) return(NULL)
      scope <- .crucible_scope(config$timeout, cancellation); scope$check()
      result <- send(op, prepare(op, values), scope)
      output <- .crucible_codec(result, op$output); scope$check()
      token <- result
      for (part in pagination$output) token <- if (is.list(token)) token[[part]] else NULL
      if (is.null(token) || identical(token, '')) finished <<- TRUE else {
        token <- .crucible_string(token); cycle <<- token %in% seen
        seen <<- c(seen, token); values[[pagination$input]] <<- token
      }
      output
    }
  }
  client
}
