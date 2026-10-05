args <- commandArgs(trailingOnly = TRUE)
library(weather.client, lib.loc = args[1L])
ns <- asNamespace('weather.client')
codec <- get('.crucible_codec', ns)
schema <- get('.crucible_schema', ns)
is_object <- get('.crucible_is_object', ns)
check <- function(value, message) if (!isTRUE(value)) stop(message, call. = FALSE)
# Keep coefficients as strings so the comparison cannot round large integers.
number <- function(text) {
  negative <- startsWith(text, '-'); text <- sub('^-', '', text)
  parts <- strsplit(tolower(text), 'e', fixed = TRUE)[[1L]]
  power <- if (length(parts) == 2L) as.integer(parts[2L]) else 0L
  decimal <- strsplit(parts[1L], '.', fixed = TRUE)[[1L]]
  power <- power - if (length(decimal) == 2L) nchar(decimal[2L]) else 0L
  digits <- sub('^0+', '', paste0(decimal, collapse = ''))
  if (digits == '') return(list('0', 0L))
  trimmed <- sub('0+$', '', digits); power <- power + nchar(digits) - nchar(trimmed)
  list(paste0(if (negative) '-' else '', trimmed), power)
}
equal_json <- function(a, b) {
  if (inherits(a, 'crucible_number') || inherits(b, 'crucible_number'))
    return(inherits(a, 'crucible_number') && inherits(b, 'crucible_number') && identical(number(as.character(a)), number(as.character(b))))
  if (is_object(a) || is_object(b)) {
    if (!is_object(a) || !is_object(b) || !setequal(names(a), names(b)) || length(a) != length(b)) return(FALSE)
    return(all(vapply(names(a), function(key) equal_json(a[[key]], b[[key]]), TRUE)))
  }
  if (is.list(a) || is.list(b)) {
    if (!is.list(a) || !is.list(b) || length(a) != length(b)) return(FALSE)
    return(all(vapply(seq_along(a), function(i) equal_json(a[[i]], b[[i]]), TRUE)))
  }
  identical(a, b)
}
rows <- jsonlite::fromJSON(Sys.getenv('SERIALIZATION_CASES'), simplifyVector = FALSE)
check(length(rows) >= 15L, 'Incomplete corpus')
for (row in rows) tryCatch({
  wire <- json_parse(row$wire)
  if (isTRUE(row$reject)) {
    rejected <- tryCatch({ codec(wire, row$target); FALSE }, error = function(e) TRUE)
    check(rejected, 'Invalid wire value was accepted')
  } else {
    native <- codec(wire, row$target); expect <- row$expect
    encoded <- json_parse(json_encode(codec(native, row$target, TRUE)))
    check(equal_json(encoded, json_parse(expect$json)), 'Exact structural JSON mismatch')
    for (member in names(expect$states)) {
      attribute <- schema$shapes[[row$target]]$members[[member]]$attribute
      check(!is.null(attribute) && attribute %in% names(native), 'Unknown observed member')
      value <- native[[attribute]]
      state <- if (inherits(value, 'crucible_unset')) 'absent' else if (is.null(value)) 'null' else 'value'
      check(identical(state, expect$states[[member]]), paste('Native presence mismatch:', member))
    }
    if (!is.null(expect$blobHex)) {
      check(is.raw(native), 'Native blob is not raw bytes')
      check(identical(paste(sprintf('%02x', as.integer(native)), collapse = ''), expect$blobHex), 'Native blob mismatch')
    }
    if (!is.null(expect$instant)) {
      member <- expect$instant$member
      attribute <- schema$shapes[[row$target]]$members[[member]]$attribute
      value <- native[[attribute]]
      check(inherits(value, 'crucible_timestamp'), 'Native timestamp type mismatch')
      check(as.character(value$seconds) == expect$instant$seconds && value$nanoseconds == expect$instant$nanoseconds, 'Native timestamp mismatch')
    }
  }
}, error = function(e) stop(row$id, ': ', conditionMessage(e), call. = FALSE))
cat('R shared serialization corpus passed\n')
