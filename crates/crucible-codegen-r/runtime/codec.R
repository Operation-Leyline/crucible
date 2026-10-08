.crucible_setting <- function(name) .crucible_schema[['protocolSettings']][[name]]
.crucible_shape <- function(target) {
  shape <- .crucible_schema$shapes[[target]]
  if (is.null(shape)) stop('Unknown model shape: ', target, call. = FALSE)
  shape
}
.crucible_document <- function(value, target, encode = FALSE) {
  if (is.null(value)) return(NULL)
  shape <- .crucible_shape(target)
  if (shape$kind %in% c('structure', 'union')) {
    if (!.crucible_is_object(value)) stop('Expected document object', call. = FALSE)
    result <- json_object(); known <- character(); wire_names <- character()
    for (name in names(shape$members)) {
      member <- shape$members[[name]]; wire <- member$jsonName
      if (is.null(wire)) wire <- name
      source <- if (encode) name else wire
      known <- c(known, source); wire_names <- c(wire_names, wire)
      if (source %in% names(value)) result[if (encode) wire else name] <- list(.crucible_document(value[[source]], member$target, encode))
    }
    if (shape$kind == 'union') for (name in setdiff(names(value), known)) {
      if (encode && name %in% wire_names) stop('Unknown variant name is declared', call. = FALSE)
      result[name] <- value[name]
    }
    return(result)
  }
  if (shape$kind == 'list') return(lapply(value, .crucible_document, target = shape$member$target, encode = encode))
  if (shape$kind == 'map') return(.crucible_object(lapply(value, .crucible_document, target = shape$value$target, encode = encode)))
  value
}
.crucible_model <- function(target, members) structure(members, class = c(.crucible_shape(target)$name, 'crucible_model', 'list'), crucible_target = target)
.crucible_union <- function(target, member, value) {
  member <- .crucible_string(member)
  structure(list(member = member, value = value), class = c(.crucible_shape(target)$name, 'crucible_union', 'list'), crucible_target = target)
}
.crucible_number_value <- function(value) {
  if (inherits(value, 'crucible_number') || inherits(value, 'crucible_long')) return(json_number(as.character(value)))
  if (is.numeric(value) && length(value) == 1L && is.finite(value)) return(json_number(sprintf('%.17g', value)))
  stop('Expected a finite number', call. = FALSE)
}
.crucible_canonical_number <- function(value) {
  text <- as.character(.crucible_number_value(value)); parts <- strsplit(tolower(text), 'e', fixed = TRUE)[[1L]]
  exponent <- if (length(parts) == 2L) parts[2L] else '0'
  negative <- startsWith(parts[1L], '-'); coefficient <- sub('^-', '', parts[1L])
  fraction <- if (grepl('.', coefficient, fixed = TRUE)) nchar(sub('^[^.]*[.]', '', coefficient)) else 0L
  coefficient <- sub('^0+', '', gsub('.', '', coefficient, fixed = TRUE))
  if (coefficient == '') return(c('0', '0'))
  trimmed <- sub('0+$', '', coefficient); adjustment <- nchar(coefficient) - nchar(trimmed) - fraction
  coefficient <- paste0(if (negative) '-' else '', trimmed)
  normalize <- function(x) { sign <- startsWith(x, '-'); digits <- sub('^0+', '', sub('^[+-]', '', x)); if (digits == '') '0' else paste0(if (sign) '-' else '', digits) }
  exponent <- normalize(exponent)
  step <- function(x, up) {
    negative <- startsWith(x, '-'); digits <- strsplit(sub('^-', '', x), '', fixed = TRUE)[[1L]]
    add <- up != negative
    if (!add && x == '0') return('-1')
    carry <- 1L; index <- length(digits)
    while (index > 0L && carry != 0L) {
      digit <- as.integer(digits[index]) + if (add) carry else -carry
      carry <- if (digit > 9L || digit < 0L) 1L else 0L
      digits[index] <- as.character((digit + 10L) %% 10L); index <- index - 1L
    }
    if (carry != 0L) digits <- c('1', digits)
    normalize(paste0(if (negative) '-' else '', paste0(digits, collapse = '')))
  }
  if (adjustment != 0L) for (ignored in seq_len(abs(adjustment))) exponent <- step(exponent, adjustment > 0L)
  c(coefficient, exponent)
}
#' Constructs an exact arbitrary precision integer from a decimal string.
big_integer <- function(value) {
  number <- json_number(as.character(value)); canonical <- .crucible_canonical_number(number)
  if (startsWith(canonical[2L], '-')) stop('Expected integral arbitrary precision number', call. = FALSE)
  structure(as.character(number), class = c('crucible_big_integer', 'crucible_number', 'character'))
}
#' Constructs an exact arbitrary precision decimal from a decimal string.
big_decimal <- function(value) {
  number <- json_number(as.character(value))
  structure(as.character(number), class = c('crucible_big_decimal', 'crucible_number', 'character'))
}
.crucible_int_enum <- function(value, name) {
  value <- as.numeric(as.character(long(.crucible_number_value(value))))
  if (value < -2147483648 || value > 2147483647) stop('IntEnum value out of range', call. = FALSE)
  structure(value, class = c(name, 'crucible_int_enum', 'numeric'))
}
.crucible_value_key <- function(value) {
  if (inherits(value, 'crucible_number') || is.numeric(value)) return(paste0('n', json_encode(as.list(.crucible_canonical_number(value)))))
  if (.crucible_is_object(value)) {
    keys <- sort(as.character(names(value)), method = 'radix')
    return(paste0('o', json_encode(lapply(keys, function(key) list(key, .crucible_value_key(value[[key]]))))))
  }
  if (is.list(value)) return(paste0('a', json_encode(lapply(value, .crucible_value_key))))
  paste0('v', json_encode(value))
}
.crucible_codec <- function(value, target, encode = FALSE, format = NULL, depth = 0L) {
  if (depth > 255L) stop('Model nesting exceeds 255 levels', call. = FALSE)
  shape <- .crucible_shape(target); kind <- shape$kind
  child <- function(value, description) .crucible_codec(value, description$target, encode, description$format, depth + 1L)
  if (is.null(value) && kind != 'document') stop('Unexpected null for ', target, call. = FALSE)
  if (kind == 'document') { json_encode(value); return(value) }
  if (kind == 'unit') { if (!.crucible_is_object(value) || length(value)) stop('Expected unit object', call. = FALSE); return(json_object()) }
  if (kind == 'string') { if (inherits(value,'crucible_number') || inherits(value,'crucible_long')) stop('Expected string, not number', call. = FALSE); return(.crucible_string(value)) }
  if (kind == 'boolean') { if (!is.logical(value) || length(value) != 1L || is.na(value)) stop('Expected boolean', call. = FALSE); return(value) }
  if (kind %in% c('bigInteger', 'bigDecimal')) {
    expected <- if (kind == 'bigInteger') 'crucible_big_integer' else 'crucible_big_decimal'
    if (encode && !inherits(value, expected)) stop('Expected exact arbitrary precision wrapper', call. = FALSE)
    result <- if (kind == 'bigInteger') big_integer(.crucible_number_value(value)) else big_decimal(.crucible_number_value(value))
    return(if (encode) json_number(as.character(result)) else result)
  }
  if (kind == 'intEnum') {
    if (encode && !inherits(value, shape$name)) stop('Wrong intEnum type', call. = FALSE)
    result <- structure(.crucible_int_enum(value, shape$name), crucible_target = target)
    return(if (encode) as.numeric(result) else result)
  }
  if (kind %in% c('byte', 'short', 'integer', 'long')) {
    if (encode && kind == 'long' && !inherits(value, 'crucible_long') && (!is.numeric(value) || abs(value) > 9007199254740991)) stop('Expected long() or exactly representable integer', call. = FALSE)
    result <- long(.crucible_number_value(value))
    if (kind == 'long') return(if (encode) json_number(as.character(result)) else result)
    result <- as.numeric(as.character(result)); bounds <- switch(kind, byte = c(-128,127), short = c(-32768,32767), integer = c(-2147483648,2147483647))
    if (result < bounds[1L] || result > bounds[2L]) stop('Integer out of range', call. = FALSE)
    return(result)
  }
  if (kind %in% c('float', 'double')) {
    # AWS JSON protocols carry nonfinite values as these JSON strings.
    if (isTRUE(.crucible_setting('nonFiniteFloatStrings'))) {
      if (encode && is.numeric(value) && length(value) == 1L && !is.object(value) && !is.finite(value)) return(if (is.nan(value)) 'NaN' else if (value > 0) 'Infinity' else '-Infinity')
      if (!encode && .crucible_query_is_string(value) && value %in% c('NaN', 'Infinity', '-Infinity')) return(switch(value, `NaN` = NaN, Infinity = Inf, -Inf))
    }
    result <- suppressWarnings(as.numeric(as.character(.crucible_number_value(value))))
    if (kind == 'float') result <- readBin(writeBin(result, raw(), size = 4L), 'numeric', n = 1L, size = 4L)
    if (!is.finite(result)) stop('Floating point overflow', call. = FALSE)
    return(result)
  }
  if (kind == 'timestamp') {
    if (is.null(format)) format <- shape$format
    return(if (encode) .crucible_time_encode(value, format) else .crucible_time_decode(value, format))
  }
  if (kind == 'blob') {
    if (encode) { if (!is.raw(value)) stop('Expected raw blob', call. = FALSE); return(as.character(openssl::base64_encode(value))) }
    text <- .crucible_string(value)
    if (!grepl('^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$', text, perl = TRUE)) stop('Invalid padded base64', call. = FALSE)
    decoded <- openssl::base64_decode(text)
    if (as.character(openssl::base64_encode(decoded)) != text) stop('Noncanonical base64', call. = FALSE)
    return(decoded)
  }
  if (kind == 'enum') {
    if (encode && !inherits(value, shape$name)) stop('Wrong enum type', call. = FALSE)
    result <- .crucible_string(value)
    return(if (encode) as.character(result) else structure(result, class = c(shape$name, 'crucible_enum', 'character'), crucible_target = target))
  }
  if (kind == 'list') {
    if (!is.list(value) || .crucible_is_object(value)) stop('Expected list array', call. = FALSE)
    result <- lapply(value, function(item) {
      if (is.null(item)) { if (!isTRUE(shape$sparse)) stop('Dense list cannot contain null', call. = FALSE); return(NULL) }
      child(item, shape$member)
    })
    if (isTRUE(shape$set)) {
      wire <- if (encode) result else lapply(result, function(item) .crucible_codec(item, shape$member$target, TRUE, shape$member$format))
      if (anyDuplicated(vapply(wire, .crucible_value_key, ''))) stop('Set members must be unique', call. = FALSE)
    }
    return(result)
  }
  if (kind == 'map') {
    if (!.crucible_is_object(value)) stop('Expected named list map', call. = FALSE)
    if (anyDuplicated(names(value))) stop('Map keys must be unique', call. = FALSE)
    for (key in names(value)) .crucible_string(key)
    return(.crucible_object(lapply(value, function(item) {
      if (is.null(item)) { if (!isTRUE(shape$sparse)) stop('Dense map cannot contain null', call. = FALSE); return(NULL) }
      child(item, shape$value)
    })))
  }
  if (kind == 'union') {
    if (encode) {
      if (!inherits(value, shape$name) || !identical(attr(value, 'crucible_target'), target)) stop('Wrong union type', call. = FALSE)
      member <- value$member; item <- value$value
      description <- shape$members[[member]]
      if (!is.null(description) && !is.null(item)) item <- child(item, description) else if (is.null(description)) json_encode(item)
      output <- json_object(); output[member] <- list(item); return(output)
    }
    if (.crucible_is_object(value) && isTRUE(.crucible_setting('ignoreUnknownUnionType')) && !'__type' %in% names(shape$members) && '__type' %in% names(value)) value <- .crucible_object(value[names(value) != '__type'])
    if (!.crucible_is_object(value) || length(value) != 1L) stop('Union requires exactly one member', call. = FALSE)
    member <- names(value)[1L]; item <- value[[1L]]; description <- shape$members[[member]]
    if (!is.null(description) && !is.null(item)) item <- child(item, description)
    return(.crucible_union(target, member, item))
  }
  if (kind == 'structure') {
    if (encode && (!inherits(value, shape$name) || !identical(attr(value, 'crucible_target'), target))) stop('Wrong model type for ', target, call. = FALSE)
    if (!is.list(value) || (!encode && !.crucible_is_object(value))) stop('Expected structure object', call. = FALSE)
    output <- json_object(); omit_null <- isTRUE(.crucible_setting('omitNullStructureMembers'))
    for (name in names(shape$members)) {
      description <- shape$members[[name]]; attribute <- description$attribute; optional <- isTRUE(description$optional)
      key <- if (encode) attribute else name
      present <- key %in% names(value); item <- value[[key]]
      if (present && is.null(item) && omit_null && .crucible_shape(description$target)$kind != 'document') present <- FALSE
      if (encode) {
        if (!present || inherits(item, 'crucible_unset')) { if (!optional) stop('Missing required member: ', name, call. = FALSE); next }
      } else if (!present) {
        item <- description$default
        if (is.null(item)) { if (!optional) stop('Missing required member: ', name, call. = FALSE); output[attribute] <- list(UNSET); next }
      }
      decoded <- if (optional && is.null(item)) NULL else child(item, description)
      output[if (encode) name else attribute] <- list(decoded)
    }
    return(if (encode) output else .crucible_model(target, unclass(output)))
  }
  stop('Unsupported model kind: ', kind, call. = FALSE)
}
.crucible_bound <- function(value) {
  if (inherits(value, 'crucible_number')) return(as.character(value))
  if (is.character(value)) return(.crucible_string(value))
  if (is.numeric(value) || is.logical(value)) return(json_encode(value))
  if (is.list(value) && !.crucible_is_object(value)) return(paste(vapply(value, function(item) {
    text <- .crucible_bound(item)
    if (text == '' || grepl('[,"\\\\]', text) || trimws(text) != text) json_encode(text) else text
  }, ''), collapse = ', '))
  stop('Expected bound scalar or list', call. = FALSE)
}
.crucible_header <- function(description, text, format = NULL) {
  shape <- .crucible_shape(description$target)
  if (shape$kind == 'list') {
    if (trimws(text) == '') return(list())
    item <- .crucible_shape(shape$member$target)
    item_format <- if (!is.null(format)) format else if (!is.null(shape$member$format)) shape$member$format else item$format
    http_date <- item$kind == 'timestamp' && identical(item_format, 'http-date')
    parts <- list(); part <- ''; quoted <- FALSE; escaped <- FALSE
    append <- function() { value <- trimws(part); if (startsWith(value, '"')) value <- json_parse(value); parts[[length(parts) + 1L]] <<- .crucible_header(shape$member, value, format); part <<- '' }
    for (char in strsplit(text, '', fixed = TRUE)[[1L]]) {
      # The comma after an HTTP-date weekday belongs to the date, not the list.
      if (char == ',' && !quoted && !(http_date && grepl('^[A-Za-z]{3}$', trimws(part)))) { append(); next }
      part <- paste0(part, char)
      if (escaped) escaped <- FALSE else if (char == '\\' && quoted) escaped <- TRUE else if (char == '"') quoted <- !quoted
    }
    if (quoted) stop('Unterminated quoted header', call. = FALSE)
    append(); return(parts)
  }
  if (shape$kind %in% c('byte','short','integer','long','float','double','boolean','intEnum','bigInteger','bigDecimal') || (shape$kind == 'timestamp' && identical(if (is.null(format)) description$format else format, 'epoch-seconds'))) return(json_parse(text))
  text
}
