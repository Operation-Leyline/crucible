# Identities, credential providers, and HTTP authentication schemes.
#' Constructs HTTP basic credentials; print and format redact secrets.
basic_credentials <- function(username, password) {
  username <- .crucible_string(username); password <- .crucible_string(password)
  if (grepl(':', username, fixed = TRUE)) stop('Basic authentication username cannot contain a colon', call. = FALSE)
  structure(list(username = username, password = password), class = 'crucible_basic_credentials')
}
#' Constructs SigV4 credentials; print and format redact secrets.
aws_credentials <- function(access_key_id, secret_access_key, session_token = NULL, expires_at = NULL) {
  access_key_id <- .crucible_string(access_key_id); secret_access_key <- .crucible_string(secret_access_key)
  if (access_key_id == '' || secret_access_key == '') stop('AWS access key and secret key must be nonempty', call. = FALSE)
  if (!is.null(session_token)) session_token <- .crucible_string(session_token)
  if (!is.null(expires_at) && (!inherits(expires_at, 'POSIXct') || length(expires_at) != 1L || is.na(expires_at))) stop('Credential expiry must be one POSIXct time', call. = FALSE)
  structure(list(access_key_id = access_key_id, secret_access_key = secret_access_key, session_token = session_token, expires_at = expires_at), class = 'crucible_aws_credentials')
}
# Secrets never appear in printed or formatted output.
format.crucible_aws_credentials <- function(x, ...) 'AwsCredentials(<redacted>)'
format.crucible_basic_credentials <- function(x, ...) 'BasicCredentials(<redacted>)'
print.crucible_aws_credentials <- function(x, ...) { cat(format(x), '\n', sep = ''); invisible(x) }
print.crucible_basic_credentials <- function(x, ...) { cat(format(x), '\n', sep = ''); invisible(x) }
# Providers are tried in order; NULL means unavailable, and an error stops the chain.
#' Tries credential providers in order until one supplies credentials.
credential_chain <- function(...) {
  providers <- list(...)
  if (any(!vapply(providers, is.function, TRUE))) stop('Credential providers must be functions', call. = FALSE)
  function() {
    for (provider in providers) { value <- provider(); if (!is.null(value)) return(value) }
    NULL
  }
}
.crucible_static_credentials <- function(access, secret, token) {
  if (access == '' && secret == '') return(NULL)
  if (access == '' || secret == '') stop('AWS credentials are incomplete', call. = FALSE)
  aws_credentials(access, secret, if (is.null(token) || token == '') NULL else token)
}
#' Reads AWS credentials from standard environment variables.
environment_credentials <- function() .crucible_static_credentials(Sys.getenv('AWS_ACCESS_KEY_ID'), Sys.getenv('AWS_SECRET_ACCESS_KEY'), Sys.getenv('AWS_SESSION_TOKEN'))
# Reads static shared-file credentials; credential_process is never executed.
#' Reads AWS credentials from a named shared credentials profile.
profile_credentials <- function(profile = NULL, path = NULL) {
  if (is.null(profile)) profile <- Sys.getenv('AWS_PROFILE', 'default')
  if (profile == '') profile <- 'default'
  if (is.null(path)) path <- Sys.getenv('AWS_SHARED_CREDENTIALS_FILE', file.path(path.expand('~'), '.aws', 'credentials'))
  if (!file.exists(path)) return(NULL)
  section <- ''; values <- list()
  for (line in trimws(readLines(path, warn = FALSE, encoding = 'UTF-8'))) {
    if (line == '' || startsWith(line, '#') || startsWith(line, ';')) next
    if (startsWith(line, '[') && endsWith(line, ']')) { section <- trimws(substr(line, 2L, nchar(line) - 1L)); next }
    if (section != profile) next
    if (!grepl('=', line, fixed = TRUE)) stop('Invalid credentials profile entry', call. = FALSE)
    key <- trimws(sub('=.*$', '', line)); value <- trimws(sub('^[^=]*=', '', line))
    if (!is.null(values[[key]])) stop('Duplicate credentials profile entry', call. = FALSE)
    values[[key]] <- value
  }
  text <- function(key) if (is.null(values[[key]])) '' else values[[key]]
  .crucible_static_credentials(text('aws_access_key_id'), text('aws_secret_access_key'), values[['aws_session_token']])
}
# Reuses an identity until shortly before it expires. R evaluates one call at a
# time per session, so a refresh cannot race another caller in that session.
#' Caches provider credentials, refreshing when expired.
cached_credentials <- function(provider, refresh_before = 60, clock = Sys.time) {
  if (!is.function(provider) || !is.function(clock)) stop('Provider and clock must be functions', call. = FALSE)
  if (!is.numeric(refresh_before) || length(refresh_before) != 1L || !is.finite(refresh_before) || refresh_before < 0) stop('refresh_before must be finite and nonnegative', call. = FALSE)
  value <- NULL; refresh_at <- NULL
  function() {
    now <- clock()
    if (!is.null(value) && (is.null(refresh_at) || refresh_at > now)) return(value)
    next_value <- provider()
    if (!is.null(next_value) && !inherits(next_value, 'crucible_aws_credentials')) stop('Credential provider must return aws_credentials() or NULL', call. = FALSE)
    expires <- next_value$expires_at
    if (!is.null(expires) && expires <= now) stop('Credential provider returned expired credentials', call. = FALSE)
    refresh_at <<- if (is.null(expires)) NULL else expires - min(refresh_before, as.numeric(difftime(expires, now, units = 'secs')) / 2)
    value <<- next_value
    value
  }
}
.crucible_hex <- function(bytes) paste(sprintf('%02x', as.integer(bytes)), collapse = '')
.crucible_hmac <- function(key, text) as.raw(openssl::sha256(charToRaw(enc2utf8(text)), key = key))
# RFC 3986 unreserved characters stay literal; everything else is %XX encoded.
.crucible_uri_encode <- function(text, slash = FALSE) {
  if (!slash) return(curl::curl_escape(text))
  # The sentinel segment keeps empty trailing segments through strsplit.
  encoded <- paste(vapply(strsplit(paste0(text, '/x'), '/', fixed = TRUE)[[1L]], curl::curl_escape, ''), collapse = '/')
  sub('/x$', '', encoded)
}
.crucible_url_parts <- function(url) {
  m <- regmatches(url, regexec('^([A-Za-z][A-Za-z0-9+.-]*)://([^/?#]+)([^?#]*)([?]([^#]*))?', url))[[1L]]
  if (!length(m)) stop('Absolute HTTP(S) URL required', call. = FALSE)
  list(scheme = tolower(m[2L]), authority = m[3L], path = m[4L], query = m[6L])
}
# Signs the final buffered request with AWS Signature Version 4.
.crucible_sign_v4 <- function(request, credentials, service, region, now = Sys.time(), unsigned_payload = FALSE, disable_double_encoding = NULL, disable_normalize_path = NULL) {
  if (!inherits(credentials, 'crucible_aws_credentials')) stop('SigV4 requires aws_credentials()', call. = FALSE)
  if (!is.null(credentials$expires_at) && credentials$expires_at <= now) stop('AWS credentials have expired', call. = FALSE)
  if (!is.character(service) || !is.character(region) || service == '' || region == '' || grepl('[/\r\n]', paste0(service, region))) stop('SigV4 service and region must be nonempty scope components', call. = FALSE)
  parts <- .crucible_url_parts(request$url)
  authority <- tolower(parts$authority)
  if (parts$scheme == 'https') authority <- sub(':443$', '', authority) else authority <- sub(':80$', '', authority)
  headers <- request$headers
  headers[['authorization']] <- NULL
  headers[['host']] <- authority
  stamp <- format(as.POSIXct(now), '%Y%m%dT%H%M%SZ', tz = 'UTC'); date <- substr(stamp, 1L, 8L)
  headers[['x-amz-date']] <- stamp
  headers[['x-amz-security-token']] <- credentials$session_token
  digest <- if (isTRUE(unsigned_payload)) 'UNSIGNED-PAYLOAD' else as.character(openssl::sha256(if (is.null(request$body)) raw() else request$body))
  headers[['x-amz-content-sha256']] <- digest
  # S3 preserves repeated slashes and escaped bytes; other services normalize
  # the path and URI-encode its percent escapes again.
  path <- if (parts$path == '') '/' else parts$path
  if (!(if (is.null(disable_normalize_path)) service == 's3' else isTRUE(disable_normalize_path))) {
    trailing <- endsWith(path, '/'); kept <- character()
    for (segment in strsplit(path, '/', fixed = TRUE)[[1L]]) {
      if (segment %in% c('', '.')) next
      if (segment == '..') { if (length(kept)) kept <- head(kept, -1L); next }
      kept <- c(kept, segment)
    }
    path <- paste0('/', paste(kept, collapse = '/'))
    if (trailing && !endsWith(path, '/')) path <- paste0(path, '/')
  }
  if (!(if (is.null(disable_double_encoding)) service == 's3' else isTRUE(disable_double_encoding))) path <- .crucible_uri_encode(path, TRUE)
  pairs <- Filter(nzchar, strsplit(parts$query, '&', fixed = TRUE)[[1L]])
  keys <- vapply(pairs, function(item) curl::curl_escape(curl::curl_unescape(sub('=.*$', '', item))), '', USE.NAMES = FALSE)
  values <- vapply(pairs, function(item) if (grepl('=', item, fixed = TRUE)) curl::curl_escape(curl::curl_unescape(sub('^[^=]*=', '', item))) else '', '', USE.NAMES = FALSE)
  ordering <- order(keys, values, method = 'radix')
  canonical_query <- if (length(pairs)) paste(paste0(keys[ordering], '=', values[ordering]), collapse = '&') else ''
  excluded <- c('authorization', 'connection', 'expect', 'transfer-encoding', 'user-agent', 'x-amzn-trace-id', 'accept-encoding', 'content-length')
  signed_names <- sort(setdiff(tolower(names(headers)), excluded), method = 'radix')
  signed <- paste(signed_names, collapse = ';')
  canonical_headers <- paste0(vapply(signed_names, function(name) paste0(name, ':', gsub('[[:space:]]+', ' ', trimws(headers[[name]])), '\n'), ''), collapse = '')
  canonical <- paste(request$method, path, canonical_query, canonical_headers, signed, digest, sep = '\n')
  scope <- paste(date, region, service, 'aws4_request', sep = '/')
  string_to_sign <- paste('AWS4-HMAC-SHA256', stamp, scope, as.character(openssl::sha256(charToRaw(enc2utf8(canonical)))), sep = '\n')
  key <- charToRaw(enc2utf8(paste0('AWS4', credentials$secret_access_key)))
  for (component in c(date, region, service, 'aws4_request')) key <- .crucible_hmac(key, component)
  signature <- .crucible_hex(.crucible_hmac(key, string_to_sign))
  headers[['authorization']] <- paste0('AWS4-HMAC-SHA256 Credential=', credentials$access_key_id, '/', scope, ', SignedHeaders=', signed, ', Signature=', signature)
  request$headers <- headers
  request
}
.crucible_query_parameter <- function(url, name) {
  query <- .crucible_url_parts(url)$query
  for (item in strsplit(query, '&', fixed = TRUE)[[1L]]) if (identical(curl::curl_unescape(sub('=.*$', '', item)), name)) return(if (grepl('=', item, fixed = TRUE)) curl::curl_unescape(sub('^[^=]*=', '', item)) else '')
  NULL
}
.crucible_starts_with_scheme <- function(value, prefix) !is.null(value) && startsWith(tolower(value), paste0(tolower(prefix), ' ')) && nchar(value) > nchar(prefix) + 1L
# Credentials the caller already placed on the request bypass the provider.
.crucible_preset_auth <- function(request, scheme) {
  kind <- scheme[['kind']]
  if (identical(kind, 'apiKey')) {
    if (identical(scheme[['location']], 'query')) { value <- .crucible_query_parameter(request$url, scheme[['name']]); return(!is.null(value) && value != '') }
    value <- request$headers[[tolower(scheme[['name']])]]
    if (is.null(scheme[['scheme']])) return(!is.null(value) && value != '')
    return(.crucible_starts_with_scheme(value, scheme[['scheme']]))
  }
  prefix <- switch(kind, bearer = 'Bearer', basic = 'Basic', sigV4 = 'AWS4-HMAC-SHA256', NULL)
  !is.null(prefix) && .crucible_starts_with_scheme(request$headers[['authorization']], prefix)
}
.crucible_apply_auth <- function(request, scheme, identity, region, unsigned_payload) {
  kind <- scheme[['kind']]
  if (identical(kind, 'bearer')) {
    request$headers[['authorization']] <- paste0('Bearer ', .crucible_string(identity))
  } else if (identical(kind, 'basic')) {
    if (!inherits(identity, 'crucible_basic_credentials')) stop('basic_auth must provide basic_credentials()', call. = FALSE)
    request$headers[['authorization']] <- paste0('Basic ', openssl::base64_encode(charToRaw(enc2utf8(paste0(identity$username, ':', identity$password)))))
  } else if (identical(kind, 'apiKey')) {
    if (!.crucible_query_is_string(identity) || identity == '') stop('api_key must provide a nonempty string', call. = FALSE)
    if (identical(scheme[['location']], 'query')) {
      parts <- .crucible_url_parts(request$url); name <- scheme[['name']]
      kept <- Filter(function(item) nzchar(item) && !identical(curl::curl_unescape(sub('=.*$', '', item)), name), strsplit(parts$query, '&', fixed = TRUE)[[1L]])
      query <- paste(c(kept, paste0(curl::curl_escape(name), '=', curl::curl_escape(identity))), collapse = '&')
      request$url <- paste0(parts$scheme, '://', parts$authority, parts$path, '?', query)
    } else {
      request$headers[[tolower(scheme[['name']])]] <- paste0(if (is.null(scheme[['scheme']])) '' else paste0(scheme[['scheme']], ' '), identity)
    }
  } else if (identical(kind, 'sigV4')) {
    if (!inherits(identity, 'crucible_aws_credentials')) stop('credentials must provide aws_credentials()', call. = FALSE)
    signing_region <- if (is.null(scheme[['signingRegion']])) region else scheme[['signingRegion']]
    if (is.null(signing_region)) stop('A region is required for SigV4', call. = FALSE)
    request <- .crucible_sign_v4(request, identity, scheme[['signingName']], signing_region, unsigned_payload = unsigned_payload,
      disable_double_encoding = scheme[['disableDoubleEncoding']], disable_normalize_path = scheme[['disableNormalizePath']])
  } else stop('Unsupported authentication scheme', call. = FALSE)
  request
}
