.crucible_shape <- function(target) {
  shape <- .crucible_schema$shapes[[target]]
  if (is.null(shape)) stop('Unknown model shape: ', target, call. = FALSE)
  shape
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
.crucible_codec <- function(value, target, encode = FALSE, format = NULL, depth = 0L) {
  if (depth > 255L) stop('Model nesting exceeds 255 levels', call. = FALSE)
  shape <- .crucible_shape(target); kind <- shape$kind
  child <- function(value, description) .crucible_codec(value, description$target, encode, description$format, depth + 1L)
  if (is.null(value) && kind != 'document') stop('Unexpected null for ', target, call. = FALSE)
  if (kind == 'document') { json_encode(value); return(value) }
  if (kind == 'unit') { if (!.crucible_is_object(value) || length(value)) stop('Expected unit object', call. = FALSE); return(json_object()) }
  if (kind == 'string') { if (inherits(value,'crucible_number') || inherits(value,'crucible_long')) stop('Expected string, not number', call. = FALSE); return(.crucible_string(value)) }
  if (kind == 'boolean') { if (!is.logical(value) || length(value) != 1L || is.na(value)) stop('Expected boolean', call. = FALSE); return(value) }
  if (kind %in% c('byte', 'short', 'integer', 'long')) {
    if (encode && kind == 'long' && !inherits(value, 'crucible_long') && (!is.numeric(value) || abs(value) > 9007199254740991)) stop('Expected long() or exactly representable integer', call. = FALSE)
    result <- long(.crucible_number_value(value))
    if (kind == 'long') return(if (encode) json_number(as.character(result)) else result)
    result <- as.numeric(as.character(result)); bounds <- switch(kind, byte = c(-128,127), short = c(-32768,32767), integer = c(-2147483648,2147483647))
    if (result < bounds[1L] || result > bounds[2L]) stop('Integer out of range', call. = FALSE)
    return(result)
  }
  if (kind %in% c('float', 'double')) {
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
    return(if (encode) unclass(result) else structure(result, class = c(shape$name, 'crucible_enum', 'character')))
  }
  if (kind == 'list') {
    if (!is.list(value) || .crucible_is_object(value)) stop('Expected list array', call. = FALSE)
    return(lapply(value, child, description = shape$member))
  }
  if (kind == 'map') {
    if (!.crucible_is_object(value)) stop('Expected named list map', call. = FALSE)
    if (anyDuplicated(names(value))) stop('Map keys must be unique', call. = FALSE)
    for (key in names(value)) .crucible_string(key)
    return(.crucible_object(lapply(value, child, description = shape$value)))
  }
  if (kind == 'union') {
    if (encode) {
      if (!inherits(value, shape$name) || !identical(attr(value, 'crucible_target'), target)) stop('Wrong union type', call. = FALSE)
      member <- value$member; item <- value$value
      description <- shape$members[[member]]
      if (!is.null(description) && !is.null(item)) item <- child(item, description) else if (is.null(description)) json_encode(item)
      output <- json_object(); output[member] <- list(item); return(output)
    }
    if (!.crucible_is_object(value) || length(value) != 1L) stop('Union requires exactly one member', call. = FALSE)
    member <- names(value)[1L]; item <- value[[1L]]; description <- shape$members[[member]]
    if (!is.null(description) && !is.null(item)) item <- child(item, description)
    return(.crucible_union(target, member, item))
  }
  if (kind == 'structure') {
    if (encode && (!inherits(value, shape$name) || !identical(attr(value, 'crucible_target'), target))) stop('Wrong model type for ', target, call. = FALSE)
    if (!is.list(value) || (!encode && !.crucible_is_object(value))) stop('Expected structure object', call. = FALSE)
    output <- json_object()
    for (name in names(shape$members)) {
      description <- shape$members[[name]]; attribute <- description$attribute; optional <- isTRUE(description$optional)
      key <- if (encode) attribute else name
      present <- key %in% names(value); item <- value[[key]]
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
.crucible_header <- function(description, text) {
  shape <- .crucible_shape(description$target)
  if (shape$kind == 'list') {
    if (trimws(text) == '') return(list())
    parts <- list(); part <- ''; quoted <- FALSE; escaped <- FALSE
    append <- function() { value <- trimws(part); if (startsWith(value, '"')) value <- json_parse(value); parts[[length(parts) + 1L]] <<- .crucible_header(shape$member, value); part <<- '' }
    for (char in strsplit(text, '', fixed = TRUE)[[1L]]) {
      if (char == ',' && !quoted) { append(); next }
      part <- paste0(part, char)
      if (escaped) escaped <- FALSE else if (char == '\\' && quoted) escaped <- TRUE else if (char == '"') quoted <- !quoted
    }
    if (quoted) stop('Unterminated quoted header', call. = FALSE)
    append(); return(parts)
  }
  if (shape$kind %in% c('byte','short','integer','long','float','double','boolean') || (shape$kind == 'timestamp' && description$format == 'epoch-seconds')) return(json_parse(text))
  text
}
