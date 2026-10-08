#' Sentinel omitting an optional model member; NULL represents explicit JSON null.
UNSET <- structure(list(), class = 'crucible_unset')
#' Constructs an exact signed 64-bit integer from a decimal string; numeric inputs beyond exact double precision are rejected.
long <- function(value) {
  if (inherits(value, 'crucible_number') || inherits(value, 'crucible_long')) value <- as.character(value)
  if (is.numeric(value)) {
    if (length(value) != 1L || !is.finite(value) || value != trunc(value) || abs(value) > 9007199254740991) stop('Use a decimal string for exact signed 64-bit integers', call. = FALSE)
    value <- sprintf('%.0f', value)
  }
  value <- .crucible_scaled(.crucible_string(value))
  negative <- startsWith(value, '-'); digits <- sub('^-', '', value)
  limit <- if (negative) '9223372036854775808' else '9223372036854775807'
  if (nchar(digits) > 19L || (nchar(digits) == 19L && digits > limit)) stop('Signed 64-bit integer out of range', call. = FALSE)
  structure(value, class = c('crucible_long', 'character'))
}
#' Constructs an exact seconds and nanoseconds timestamp; nanoseconds must be in [0, 1000000000).
timestamp <- function(seconds, nanoseconds = 0L) {
  seconds <- long(seconds)
  if (!is.numeric(nanoseconds) || length(nanoseconds) != 1L || !is.finite(nanoseconds) || nanoseconds < 0 || nanoseconds >= 1e9 || nanoseconds != trunc(nanoseconds)) stop('Nanoseconds must be an integer in [0, 1000000000)', call. = FALSE)
  structure(list(seconds = seconds, nanoseconds = as.integer(nanoseconds)), class = 'crucible_timestamp')
}
.crucible_epoch <- function(value) {
  scaled <- .crucible_scaled(value, 9L)
  negative <- startsWith(scaled, '-'); digits <- sub('^-', '', scaled)
  digits <- paste0(strrep('0', max(0L, 10L - nchar(digits))), digits)
  seconds <- substr(digits, 1L, nchar(digits) - 9L)
  nano <- as.integer(substr(digits, nchar(digits) - 8L, nchar(digits)))
  if (negative && nano > 0L) { seconds <- paste0('-', .crucible_increment(seconds)); nano <- 1e9 - nano } else if (negative) seconds <- paste0('-', seconds)
  timestamp(seconds, nano)
}
.crucible_epoch_text <- function(value) {
  seconds <- as.character(value$seconds); nano <- value$nanoseconds
  if (nano == 0L) return(seconds)
  if (startsWith(seconds, '-')) { seconds <- paste0('-', .crucible_decrement(sub('^-', '', seconds))); nano <- 1e9 - nano }
  paste0(seconds, '.', sub('0+$', '', sprintf('%09d', as.integer(nano))))
}
.crucible_days <- function(year, month, day) {
  if (year < 1 || year > 9999 || month < 1 || month > 12 || day < 1 || day > 31) stop('Invalid calendar date', call. = FALSE)
  leap <- year %% 4L == 0L && (year %% 100L != 0L || year %% 400L == 0L)
  lengths <- c(31, if (leap) 29 else 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31)
  if (day > lengths[month]) stop('Invalid calendar date', call. = FALSE)
  y <- year - if (month <= 2) 1 else 0
  era <- floor(y / 400); yoe <- y - era * 400
  doy <- floor((153 * (month + if (month > 2) -3 else 9) + 2) / 5) + day - 1
  era * 146097 + yoe * 365 + floor(yoe / 4) - floor(yoe / 100) + doy - 719468
}
.crucible_calendar <- function(value) {
  seconds <- as.numeric(as.character(value$seconds))
  if (seconds < -62135596800 || seconds > 253402300799) stop('Text timestamps require years 0001 through 9999', call. = FALSE)
  days <- floor(seconds / 86400); rest <- seconds - days * 86400
  z <- days + 719468; era <- floor(z / 146097); doe <- z - era * 146097
  yoe <- floor((doe - floor(doe / 1460) + floor(doe / 36524) - floor(doe / 146096)) / 365)
  year <- yoe + era * 400
  doy <- doe - (365 * yoe + floor(yoe / 4) - floor(yoe / 100)); mp <- floor((5 * doy + 2) / 153)
  day <- doy - floor((153 * mp + 2) / 5) + 1; month <- mp + if (mp < 10) 3 else -9
  year <- year + if (month <= 2) 1 else 0
  list(year = year, month = month, day = day, hour = floor(rest / 3600), minute = floor(rest %% 3600 / 60), second = rest %% 60, weekday = (days + 4) %% 7 + 1)
}
.crucible_time_decode <- function(value, format) {
  if (format == 'epoch-seconds') {
    if (!inherits(value, 'crucible_number') && !is.numeric(value)) stop('Epoch timestamp must be a JSON number', call. = FALSE)
    return(.crucible_epoch(if (is.numeric(value)) sprintf('%.17g', value) else value))
  }
  value <- .crucible_string(value)
  if (format == 'http-date') {
    m <- regmatches(value, regexec('^(Sun|Mon|Tue|Wed|Thu|Fri|Sat), ([0-9]{2}) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) ([0-9]{4}) ([0-9]{2}):([0-9]{2}):([0-9]{2}) GMT$', value))[[1L]]
    if (!length(m)) stop('Invalid HTTP date', call. = FALSE)
    months <- c('Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec')
    result <- .crucible_time_decode(sprintf('%s-%02d-%sT%s:%s:%sZ', m[5L], match(m[4L], months), m[3L], m[6L], m[7L], m[8L]), 'date-time')
    if (.crucible_time_encode(result, 'http-date') != value) stop('Invalid HTTP date weekday', call. = FALSE)
    return(result)
  }
  m <- regmatches(value, regexec('^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})([.][0-9]{1,9})?(Z|[+-][0-9]{2}:[0-9]{2})$', value))[[1L]]
  if (!length(m)) stop('Invalid RFC 3339 timestamp', call. = FALSE)
  n <- as.integer(m[2L:7L]); if (n[4L] > 23L || n[5L] > 59L || n[6L] > 59L) stop('Invalid timestamp clock', call. = FALSE)
  seconds <- .crucible_days(n[1L], n[2L], n[3L]) * 86400 + n[4L] * 3600 + n[5L] * 60 + n[6L]
  if (m[9L] != 'Z') { hours <- as.integer(substr(m[9L], 2L, 3L)); minutes <- as.integer(substr(m[9L], 5L, 6L)); if (hours > 23 || minutes > 59) stop('Invalid timestamp offset', call. = FALSE); seconds <- seconds - (if (startsWith(m[9L], '-')) -1 else 1) * (hours * 3600 + minutes * 60) }
  nano <- if (m[8L] == '') 0L else as.integer(paste0(sub('^[.]', '', m[8L]), strrep('0', 10L - nchar(m[8L]))))
  result <- timestamp(seconds, nano); .crucible_calendar(result); result
}
.crucible_time_encode <- function(value, format) {
  if (!inherits(value, 'crucible_timestamp')) stop('Expected timestamp()', call. = FALSE)
  if (format == 'epoch-seconds') return(json_number(.crucible_epoch_text(value)))
  p <- .crucible_calendar(value)
  if (format == 'http-date') return(sprintf('%s, %02d %s %04d %02d:%02d:%02d GMT', c('Sun','Mon','Tue','Wed','Thu','Fri','Sat')[p$weekday], p$day, c('Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec')[p$month], p$year, p$hour, p$minute, p$second))
  paste0(sprintf('%04d-%02d-%02dT%02d:%02d:%02d', p$year,p$month,p$day,p$hour,p$minute,p$second), if (value$nanoseconds) paste0('.',sub('0+$','',sprintf('%09d',value$nanoseconds))) else '', 'Z')
}
.crucible_condition <- function(class, message, ...) structure(c(list(message = message, call = NULL), list(...)), class = c(class, 'crucible_error', 'error', 'condition'))
#' Constructs a cancellation environment with cancel and check methods; an optional predicate is polled by check.
cancellation_token <- function(predicate = NULL) {
  if (!is.null(predicate) && !is.function(predicate)) stop('Cancellation predicate must be a function', call. = FALSE)
  token <- new.env(parent = emptyenv()); cancelled <- FALSE
  token$cancel <- function() { cancelled <<- TRUE; invisible(NULL) }
  token$check <- function() { if (!cancelled && !is.null(predicate)) cancelled <<- isTRUE(predicate()); if (cancelled) stop(.crucible_condition('crucible_cancelled', 'Request cancelled')); invisible(NULL) }
  token
}
.crucible_now <- function() .Call(C_crucible_now)
.crucible_scope <- function(timeout, cancellation) {
  deadline <- .crucible_now() + timeout
  scope <- new.env(parent = emptyenv())
  scope$remaining <- function() max(0, deadline - .crucible_now())
  scope$check <- function() { if (!is.null(cancellation)) cancellation$check(); if (scope$remaining() <= 0) stop(.crucible_condition('crucible_deadline', 'Request deadline exceeded')); invisible(NULL) }
  scope$wait <- function(seconds) { end <- .crucible_now() + seconds; repeat { scope$check(); left <- end - .crucible_now(); if (left <= 0) return(invisible(NULL)); Sys.sleep(min(.025, left, scope$remaining())) } }
  scope
}
