.crucible_string <- function(value) {
  if (!is.character(value) || inherits(value, 'crucible_number') || inherits(value, 'crucible_long') || length(value) != 1L || is.na(value) || !validUTF8(value)) stop('Expected one valid UTF-8 string', call. = FALSE)
  enc2utf8(value)
}
#' Preserves a JSON numeric token without double rounding.
json_number <- function(value) {
  value <- .crucible_string(value)
  if (!grepl('^-?(0|[1-9][0-9]*)([.][0-9]+)?([eE][+-]?[0-9]+)?$', value)) stop('Invalid JSON number', call. = FALSE)
  structure(value, class = c('crucible_number', 'character'))
}
#' Constructs a JSON object distinct from an unnamed array.
json_object <- function(...) {
  value <- list(...)
  if (length(value) && (is.null(names(value)) || any(names(value) == '') || anyDuplicated(names(value)))) stop('JSON object members must have unique names', call. = FALSE)
  structure(value, class = c('crucible_object', 'list'))
}
.crucible_object <- function(value = list()) {
  if (!is.list(value) || (length(value) && is.null(names(value)))) stop('Expected JSON object', call. = FALSE)
  structure(value, class = c('crucible_object', 'list'))
}
.crucible_is_object <- function(value) inherits(value, 'crucible_object') || (is.list(value) && !is.null(names(value)))
#' Parses a UTF-8 JSON string, preserving exact numeric tokens.
json_parse <- function(text) {
  if (is.raw(text)) text <- rawToChar(text)
  text <- .crucible_string(text)
  i <- 1L; size <- nchar(text, type = 'chars')
  peek <- function() if (i > size) '' else substr(text, i, i)
  whitespace <- function() { while (i <= size && peek() %in% c(' ', '\t', '\r', '\n')) i <<- i + 1L }
  fail <- function() stop('Invalid JSON at character ', i, call. = FALSE)
  string <- function() {
    begin <- i; i <<- i + 1L
    while (i <= size) {
      char <- peek()
      if (char == '"') {
        i <<- i + 1L
        token <- substr(text, begin, i - 1L)
        result <- tryCatch(jsonlite::fromJSON(token, simplifyVector = FALSE), error = function(e) fail())
        return(.crucible_string(result))
      }
      if (utf8ToInt(char) < 32L) fail()
      if (char == '\\') {
        escape <- substr(text, i + 1L, i + 1L)
        if (!escape %in% c('"', '\\', '/', 'b', 'f', 'n', 'r', 't', 'u')) fail()
        if (escape == 'u') {
          hex <- substr(text, i + 2L, i + 5L)
          if (!grepl('^[0-9A-Fa-f]{4}$', hex)) fail()
          point <- strtoi(hex, 16L)
          # jsonlite replaces some unpaired surrogates rather than rejecting them.
          if (point == 0L) stop('R strings cannot represent NUL', call. = FALSE)
          if (point >= 55296L && point <= 56319L) {
            low <- substr(text, i + 8L, i + 11L)
            if (substr(text, i + 6L, i + 7L) != '\\u' || !grepl('^[dD][c-fC-F][0-9A-Fa-f]{2}$', low)) fail()
            i <<- i + 6L
          } else if (point >= 56320L && point <= 57343L) fail()
          i <<- i + 4L
        }
        i <<- i + 1L
      }
      i <<- i + 1L
    }
    fail()
  }
  value <- function(depth = 0L) {
    if (depth > 255L) stop('JSON nesting exceeds 255 levels', call. = FALSE)
    whitespace(); char <- peek()
    if (char == '"') return(string())
    if (char %in% c('{', '[')) {
      object <- char == '{'; end <- if (object) '}' else ']'
      i <<- i + 1L; whitespace(); output <- if (object) json_object() else list()
      if (peek() == end) { i <<- i + 1L; return(output) }
      repeat {
        whitespace()
        if (object) { if (peek() != '"') fail(); key <- string(); whitespace(); if (peek() != ':') fail(); i <<- i + 1L }
        item <- value(depth + 1L)
        if (object) output[key] <- list(item) else output[length(output) + 1L] <- list(item)
        whitespace()
        if (peek() == end) { i <<- i + 1L; return(output) }
        if (peek() != ',') fail()
        i <<- i + 1L
      }
    }
    for (keyword in c('true', 'false', 'null')) {
      if (substr(text, i, i + nchar(keyword) - 1L) == keyword) {
        i <<- i + nchar(keyword)
        return(switch(keyword, true = TRUE, false = FALSE, null = NULL))
      }
    }
    rest <- substr(text, i, size)
    match <- regexpr('^-?(0|[1-9][0-9]*)([.][0-9]+)?([eE][+-]?[0-9]+)?', rest)
    if (match[1L] != 1L) fail()
    n <- attr(match, 'match.length'); i <<- i + n
    json_number(substr(rest, 1L, n))
  }
  result <- value(); whitespace(); if (i <= size) fail()
  result
}
#' Encodes a native JSON value into UTF-8 text.
json_encode <- function(value) {
  encode <- function(x, depth) {
    if (depth > 255L) stop('JSON nesting exceeds 255 levels', call. = FALSE)
    if (is.null(x)) return('null')
    if (inherits(x, 'crucible_number') || inherits(x, 'crucible_long')) return(as.character(json_number(as.character(x))))
    if (is.character(x) && length(x) == 1L) return(as.character(jsonlite::toJSON(.crucible_string(x), auto_unbox = TRUE)))
    if (is.logical(x) && length(x) == 1L && !is.na(x)) return(if (x) 'true' else 'false')
    if (is.numeric(x) && length(x) == 1L && is.finite(x)) return(as.character(json_number(sprintf('%.17g', x))))
    if (is.list(x)) {
      if (.crucible_is_object(x)) {
        keys <- names(x)
        entries <- vapply(seq_along(x), function(i) paste0(encode(keys[i], depth + 1L), ':', encode(x[[i]], depth + 1L)), '')
        return(paste0('{', paste(entries, collapse = ','), '}'))
      }
      return(paste0('[', paste(vapply(x, encode, '', depth = depth + 1L), collapse = ','), ']'))
    }
    stop('Unsupported JSON value; use lists for arrays and json_object() for objects', call. = FALSE)
  }
  encode(value, 0L)
}
.crucible_scaled <- function(text, scale = 0L) {
  text <- as.character(json_number(as.character(text)))
  negative <- startsWith(text, '-'); text <- sub('^-', '', text)
  parts <- strsplit(tolower(text), 'e', fixed = TRUE)[[1L]]
  exponent <- if (length(parts) == 2L) suppressWarnings(as.numeric(parts[2L])) else 0
  if (!is.finite(exponent) || abs(exponent) > 10000) stop('JSON exponent exceeds supported exact range', call. = FALSE)
  decimal <- strsplit(parts[1L], '.', fixed = TRUE)[[1L]]
  digits <- paste0(decimal, collapse = '')
  shift <- exponent + scale - if (length(decimal) == 2L) nchar(decimal[2L]) else 0L
  if (shift < 0) {
    split <- nchar(digits) + shift
    discarded <- if (split < 1L) digits else substr(digits, split + 1L, nchar(digits))
    if (grepl('[1-9]', discarded)) stop('Number has unsupported fractional precision', call. = FALSE)
    digits <- if (split < 1L) '0' else substr(digits, 1L, split)
  } else if (shift > 0) digits <- paste0(digits, strrep('0', shift))
  digits <- sub('^0+', '', digits); if (digits == '') digits <- '0'
  if (negative && digits != '0') paste0('-', digits) else digits
}
.crucible_increment <- function(digits) {
  values <- as.integer(strsplit(digits, '', fixed = TRUE)[[1L]])
  for (i in rev(seq_along(values))) { if (values[i] < 9L) { values[i] <- values[i] + 1L; return(paste0(values, collapse = '')) }; values[i] <- 0L }
  paste0('1', paste0(values, collapse = ''))
}
.crucible_decrement <- function(digits) {
  values <- as.integer(strsplit(digits, '', fixed = TRUE)[[1L]])
  for (i in rev(seq_along(values))) { if (values[i] > 0L) { values[i] <- values[i] - 1L; break }; values[i] <- 9L }
  value <- sub('^0+', '', paste0(values, collapse = '')); if (value == '') '0' else value
}
