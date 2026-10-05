args <- commandArgs(trailingOnly = TRUE)
# Load lazily used namespaces up front so call deadlines measure calls, not package loading.
invisible(lapply(c('curl', 'openssl', 'jsonlite', 'stats'), loadNamespace))
suppressPackageStartupMessages(library(auth.client, lib.loc = args[1L]))
ns <- asNamespace('auth.client')
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)
rejects <- function(expression, class = 'error') { result <- tryCatch({ force(expression); NULL }, error = identity); if (is.null(result) || !inherits(result, class)) stop('Expected ', class, ', got ', paste(format(result), collapse = ' ')); invisible(result) }
sent <- list()
transport <- function(status = 204L) function(request, read_body, scope) { sent[[length(sent) + 1L]] <<- request; list(status = status, headers = list(), body = raw()) }
last <- function() sent[[length(sent)]]
forbidden <- function() stop('Credential provider must not be invoked')
sdk <- function(...) AuthClient(client_config(endpoint = 'https://example.test', transport = transport(), base_delay = 0, ...))

# Credentials the caller already supplied bypass the matching provider.
invisible(sdk(headers = list(authorization = 'Basic supplied'), basic_auth = forbidden)$read())
check(last()$headers$authorization == 'Basic supplied', 'Preset basic credentials')
invisible(sdk(headers = list('x-api-key' = 'ApiKey supplied'), api_key = forbidden)$read())
check(last()$headers[['x-api-key']] == 'ApiKey supplied', 'Preset API key')
rejects(sdk(headers = list(authorization = 'Digest unrelated'))$read())

# Modeled scheme order, anonymous operations, and optional authentication.
client <- sdk(basic_auth = basic_credentials('user', 'secret'), api_key = forbidden)
invisible(client$read()); check(last()$headers$authorization == 'Basic dXNlcjpzZWNyZXQ=', 'Basic authentication')
invisible(client$public()); check(is.null(last()$headers$authorization), 'Anonymous operation')
count <- length(sent); invisible(sdk()$optional()); check(length(sent) == count + 1L && is.null(last()$headers$authorization), 'Optional authentication')
count <- length(sent); error <- rejects(sdk()$read()); check(grepl('No credentials', conditionMessage(error)) && length(sent) == count, 'Required credentials fail before sending')
client <- sdk(api_key = function() 'secret'); invisible(client$read()); check(last()$headers[['x-api-key']] == 'ApiKey secret', 'API key header')
apply <- get('.crucible_apply_auth', ns); preset <- get('.crucible_preset_auth', ns)
scheme <- list(kind = 'apiKey', location = 'query', name = 'key')
request <- apply(list(url = 'https://example.test/read?key=old&x=1', method = 'GET', headers = list(), body = NULL), scheme, 'se cret', NULL, FALSE)
check(request$url == 'https://example.test/read?x=1&key=se%20cret' && preset(request, scheme), 'API key query parameter')
check(!preset(list(url = 'https://example.test/read', headers = list()), scheme), 'Missing query API key')
rejects(AuthClient(client_config(endpoint = 'http://example.test', transport = transport(), basic_auth = basic_credentials('user', 'secret')))$read())
invisible(AuthClient(client_config(endpoint = 'http://127.0.0.1:9', transport = transport(), basic_auth = basic_credentials('user', 'secret')))$read())
rejects(basic_credentials('us:er', 'secret')); rejects(client_config(basic_auth = 'user:secret')); rejects(client_config(api_key = ''))
printed <- paste(capture.output(print(basic_credentials('user', 'secret')), print(aws_credentials('AKID', 'secret-key', 'token'))), collapse = '\n')
check(!grepl('secret', printed) && !grepl('AKID', printed) && !grepl('token', printed), 'Printed credentials are redacted')

# Published AWS SigV4 vector: S3 single-chunk GET (public documentation credentials).
sign <- get('.crucible_sign_v4', ns)
credentials <- aws_credentials('AKIAIOSFODNN7EXAMPLE', 'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY')
signed <- sign(list(url = 'https://examplebucket.s3.amazonaws.com/test.txt', method = 'GET', headers = list(range = 'bytes=0-9'), body = NULL), credentials, 's3', 'us-east-1', now = as.POSIXct('2013-05-24 00:00:00', tz = 'UTC'))
check(signed$headers$authorization == 'AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41', 'SigV4 S3 vector')
check(signed$headers[['x-amz-content-sha256']] == 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855', 'SigV4 payload digest')
# Non-S3 services normalize the path and encode its escapes again; the query is canonically sorted.
signed <- sign(list(url = 'https://example.amazonaws.com/a/./b/../c%20d/?b=2&a=x%2Fy&a=1', method = 'GET', headers = list(), body = NULL), credentials, 'service', 'us-east-1', now = as.POSIXct('2015-08-30 12:36:00', tz = 'UTC'))
check(grepl('SignedHeaders=host;x-amz-content-sha256;x-amz-date,', signed$headers$authorization, fixed = TRUE), 'SigV4 signed header set')
expired <- aws_credentials('AKID', 'secret', expires_at = as.POSIXct('2000-01-01', tz = 'UTC'))
rejects(sign(list(url = 'https://example.test/', method = 'GET', headers = list(), body = NULL), expired, 's3', 'us-east-1'))

# Signing follows middleware and uses a refreshed identity on each attempt.
sent <- list(); identities <- list(aws_credentials('first', 'secret', 'session-one'), aws_credentials('second', 'secret', 'session-two'))
retry <- function(request, read_body, scope) {
  sent[[length(sent) + 1L]] <<- request
  if (length(sent) == 1L) list(status = 503L, headers = list(), body = charToRaw('{"__type":"Unavailable"}')) else list(status = 204L, headers = list(), body = raw())
}
client <- AuthClient(client_config(endpoint = 'https://example.test', transport = retry, base_delay = 0, region = 'us-east-1',
  credentials = function() { value <- identities[[1L]]; identities <<- identities[-1L]; value },
  middleware = list(function(request) { request$headers[['x-custom']] <- 'value'; request })))
invisible(client$read())
check(length(sent) == 2L && grepl('Credential=second/', sent[[2L]]$headers$authorization, fixed = TRUE), 'Refreshed SigV4 identity')
check(grepl('x-custom', sent[[2L]]$headers$authorization, fixed = TRUE) && sent[[2L]]$headers[['x-amz-security-token']] == 'session-two', 'Signed middleware header and session token')
rejects(AuthClient(client_config(endpoint = 'https://example.test', transport = transport(), credentials = credentials))$read())

# Credential providers: caching, refresh windows, chains, environment, and profiles.
now <- as.POSIXct('2026-01-01 00:00:00', tz = 'UTC'); calls <- 0L
provider <- function() { calls <<- calls + 1L; aws_credentials('key', 'secret', expires_at = now + 300) }
cached <- cached_credentials(provider, clock = function() now)
first <- cached(); for (i in 1:30) check(identical(cached(), first), 'Cached identity reused')
check(calls == 1L, 'One provider call while fresh')
now <- now + 270; check(!identical(cached()$expires_at, first$expires_at) && calls == 2L, 'Refresh inside the window')
short_calls <- 0L
short <- cached_credentials(function() { short_calls <<- short_calls + 1L; aws_credentials('key', 'secret', expires_at = now + 20) }, clock = function() now)
for (i in 1:30) short()
check(short_calls == 1L, 'Short-lived credentials remain reusable')
check(identical(credential_chain(function() NULL, cached)(), cached()), 'Credential chain')
rejects(credential_chain(function() stop('broken'), cached)())
rejects(cached_credentials(function() aws_credentials('key', 'secret', expires_at = now), clock = function() now)())
rejects(cached_credentials(function() 'not credentials')())
Sys.setenv(AWS_ACCESS_KEY_ID = 'env', AWS_SECRET_ACCESS_KEY = 'secret', AWS_SESSION_TOKEN = 'token')
check(environment_credentials()$access_key_id == 'env' && environment_credentials()$session_token == 'token', 'Environment credentials')
Sys.setenv(AWS_SECRET_ACCESS_KEY = ''); rejects(environment_credentials())
Sys.unsetenv(c('AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN')); check(is.null(environment_credentials()), 'No environment credentials')
path <- tempfile(); writeLines(c('[default]', 'aws_access_key_id = other', 'aws_secret_access_key = other', '[test]', 'aws_access_key_id=profile', 'aws_secret_access_key=secret%with-percent', 'credential_process = touch /tmp/never'), path)
check(profile_credentials('test', path)$secret_access_key == 'secret%with-percent', 'Profile credentials')
check(is.null(profile_credentials('missing', path)) && is.null(profile_credentials('test', tempfile())), 'Missing profile')
writeLines(c('[test]', 'aws_access_key_id=a', 'aws_access_key_id=b', 'aws_secret_access_key=s'), path); rejects(profile_credentials('test', path))

if (nzchar(Sys.getenv('ECHO_URL'))) {
  # SigV4 and basic authentication over native HTTP to the loopback fixture.
  requests <- function() jsonlite::fromJSON(rawToChar(curl::curl_fetch_memory(paste0(Sys.getenv('ECHO_URL'), '/__requests'))$content), simplifyVector = FALSE)
  client <- AuthClient(client_config(endpoint = Sys.getenv('ECHO_URL'), credentials = cached_credentials(function() credentials), region = 'us-east-1'))
  invisible(client$read()); received <- requests(); received <- received[[length(received)]]
  check(startsWith(received$headers$authorization, 'AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/') && received$path == '/read', 'Native HTTP SigV4')
  check(received$headers$host == sub('^http://', '', Sys.getenv('ECHO_URL')), 'Signed host header is transmitted')
  client$close()
  client <- AuthClient(client_config(endpoint = Sys.getenv('ECHO_URL'), basic_auth = function() basic_credentials('user', 'secret')))
  invisible(client$read()); received <- requests(); check(received[[length(received)]]$headers$authorization == 'Basic dXNlcjpzZWNyZXQ=', 'Native HTTP basic auth')
  client$close()
}
cat('R authentication and credential providers passed\n')
