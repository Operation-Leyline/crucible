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
  for (name in bindings$labels) {
    value <- .crucible_bound(values[[name]])
    if (value == '') stop('Empty HTTP label: ', name, call. = FALSE)
    path <- gsub(paste0('{', name, '}'), curl::curl_escape(value), path, fixed = TRUE)
  }
  query <- character()
  for (binding in bindings$query) {
    value <- values[[binding[[1L]]]]
    if (is.null(value)) next
    if (!is.list(value)) value <- list(value)
    for (item in value) query <- c(query, paste0(curl::curl_escape(binding[[2L]]), '=', curl::curl_escape(.crucible_bound(item))))
  }
  if (length(query)) path <- paste0(path, if (grepl('?', path, fixed = TRUE)) '&' else '?', paste(query, collapse = '&'))
  headers <- config$headers
  for (binding in bindings$headers) if (!is.null(values[[binding[[1L]]]])) headers[[tolower(binding[[2L]])]] <- .crucible_bound(values[[binding[[1L]]]])
  body <- NULL
  if (length(bindings$body)) {
    object <- json_object()
    for (name in bindings$body) if (name %in% names(values)) object[name] <- values[name]
    body <- charToRaw(enc2utf8(json_encode(object))); headers[['content-type']] <- 'application/json'
  }
  payload <- bindings$payload
  if (!is.null(payload) && !is.null(values[[payload[[1L]]]])) {
    bytes <- openssl::base64_decode(values[[payload[[1L]]]])
    if (length(bytes)) { body <- bytes; headers[['content-type']] <- payload[[2L]] }
  }
  .crucible_request(list(url = paste0(endpoint, path), method = op$method, headers = headers, body = body))
}
.crucible_apply_headers <- function(target, bindings, response, body) {
  for (binding in bindings) {
    value <- response$headers[[tolower(binding[[2L]])]]
    if (!is.null(value)) body[binding[[1L]]] <- list(.crucible_header(.crucible_shape(target)$members[[binding[[1L]]]], value))
  }
  body
}
.crucible_success <- function(op, response) {
  payload <- op$response$payload
  if (!is.null(payload)) { body <- json_object(); body[[payload[[1L]]]] <- as.character(openssl::base64_encode(response$body)) }
  else if (!length(op$response$body) || !length(response$body)) body <- json_object()
  else { body <- json_parse(response$body); if (!.crucible_is_object(body)) stop('Expected response object', call. = FALSE) }
  if (!is.null(op$output)) body <- .crucible_apply_headers(op$output, op$response$headers, response, body)
  body
}
.crucible_failure <- function(op, response) {
  body <- tryCatch(json_parse(response$body), error = function(e) json_object())
  if (!.crucible_is_object(body)) body <- json_object()
  name <- body[['__type']]; if (!is.character(name) || inherits(name, 'crucible_number')) name <- NULL
  matches <- Filter(function(target) { shape <- .crucible_shape(target); if (!is.null(name)) shape$name == name else as.numeric(shape$error$status) == response$status }, op$errors)
  target <- if (length(matches) == 1L) matches[[1L]] else NULL
  shape <- if (!is.null(target)) .crucible_shape(target) else NULL; traits <- shape$error
  if (is.null(name)) name <- if (!is.null(shape)) shape$name else 'UnknownError'
  message <- body[['message']]; if (!is.character(message) || inherits(message, 'crucible_number')) message <- paste('HTTP', response$status)
  request_id <- response$headers[['x-request-id']]
  if (is.null(request_id) && is.character(body[['requestId']]) && !inherits(body[['requestId']],'crucible_number')) request_id <- body[['requestId']]
  metadata <- list(name = name, message = message, status = response$status, request_id = request_id, headers = response$headers, retryable = isTRUE(traits$retryable), throttling = isTRUE(traits$throttling))
  model <- if (is.null(target)) NULL else tryCatch(.crucible_codec(.crucible_apply_headers(target, traits$headers, response, body), target), error = function(e) NULL)
  error <- .crucible_condition(c(if (!is.null(model)) shape$name, 'crucible_service_error'), message, metadata = metadata, model = model)
  error
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
.crucible_client <- function(config, endpoint, digest) {
  if (!is.null(config$endpoint)) endpoint <- config$endpoint
  if (is.null(endpoint)) stop('An endpoint is required', call. = FALSE)
  .crucible_url(endpoint)
  if (grepl('[?#]', endpoint)) stop('Endpoint cannot contain query or fragment', call. = FALSE)
  endpoint <- sub('/+$', '', endpoint)
  transport <- if (is.null(config$transport)) .crucible_transport() else config$transport
  closed <- FALSE; client <- new.env(parent = emptyenv())
  client$close <- function() { if (!closed && is.null(config$transport)) attr(transport, 'close')(); closed <<- TRUE; transport <<- NULL; invisible(NULL) }
  operation <- function(name) { op <- .crucible_schema$operations[[name]]; if (is.null(op)) stop('Unknown operation: ', name, call. = FALSE); op }
  send <- function(op, prepared, scope) {
    if (closed) stop('Client is closed', call. = FALSE)
    for (attempt in seq_len(config$max_attempts)) {
      scope$check(); request <- prepared
      if (isTRUE(.crucible_schema$bearer) && !is.null(config$token)) {
        token <- config$token(); scope$check()
        if (!is.null(token)) request$headers[['authorization']] <- paste0('Bearer ', .crucible_string(token))
      }
      for (middleware in config$middleware) { scope$check(); request <- .crucible_request(middleware(request)) }
      scope$check(); authority <- tolower(.crucible_url(request$url))
      hostname <- sub(':[0-9]+$', '', authority)
      if (!is.null(request$headers[['authorization']]) && !startsWith(request$url,'https://') && !hostname %in% c('localhost','127.0.0.1','[::1]')) stop('Credentials require HTTPS or loopback HTTP', call. = FALSE)
      if (digest) {
        request$headers[['x-amz-content-sha256']] <- NULL
        if (length(request$body)) request$headers[['x-amz-content-sha256']] <- as.character(openssl::sha256(request$body))
      }
      request <- .crucible_request(request)
      response <- tryCatch(transport(request, length(op$response$body) > 0L || !is.null(op$response$payload), scope), crucible_network = function(e) e)
      scope$check()
      if (inherits(response, 'crucible_network')) {
        if (!isTRUE(op$safe) || attempt >= config$max_attempts) stop(response)
        scope$wait(.crucible_retry_delay(config, attempt, NULL)); next
      }
      response$headers <- .crucible_headers(response$headers)
      if (response$status >= 200 && response$status < 300) return(.crucible_success(op, response))
      error <- .crucible_failure(op, response)
      retry <- error$metadata$throttling || (isTRUE(op$safe) && (error$metadata$retryable || response$status %in% c(500,502,503,504)))
      if (!retry || attempt >= config$max_attempts) stop(error)
      scope$wait(.crucible_retry_delay(config, attempt, response$headers[['retry-after']]))
    }
    stop('No request attempt', call. = FALSE)
  }
  client$call <- function(name, input = NULL, cancellation = NULL) {
    scope <- .crucible_scope(config$timeout, cancellation); scope$check(); op <- operation(name)
    values <- if (is.null(op$input)) json_object() else .crucible_codec(input, op$input, TRUE)
    result <- send(op, .crucible_prepare(op, values, config, endpoint), scope)
    output <- if (is.null(op$output)) NULL else .crucible_codec(result, op$output)
    scope$check(); output
  }
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
      result <- send(op, .crucible_prepare(op, values, config, endpoint), scope)
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
