# Evaluates the portable JMESPath tree compiled by crucible-smithy. Numbers stay
# exact decimal text (json_number); arrays are unnamed lists and objects json_object().
.crucible_query_fail <- function(message) stop(.crucible_condition('crucible_query_error', message))
.crucible_query_is_number <- function(x) inherits(x, 'crucible_number') || inherits(x, 'crucible_long') || (is.numeric(x) && length(x) == 1L && !is.object(x)) || inherits(x, 'crucible_int_enum')
.crucible_query_is_string <- function(x) is.character(x) && length(x) == 1L && !.crucible_query_is_number(x)
.crucible_query_is_bool <- function(x) is.logical(x) && length(x) == 1L && !is.na(x)
.crucible_query_is_object <- function(x) is.list(x) && .crucible_is_object(x)
.crucible_query_is_array <- function(x) is.list(x) && !.crucible_is_object(x)
.crucible_query_number <- function(x) {
  if (!.crucible_query_is_number(x)) .crucible_query_fail('JMESPath expected number')
  if (is.numeric(x)) { if (!is.finite(x)) .crucible_query_fail('JMESPath number is not finite'); return(json_number(sprintf('%.17g', as.numeric(x)))) }
  json_number(as.character(x))
}
.crucible_query_text <- function(x) { if (!.crucible_query_is_string(x)) .crucible_query_fail('JMESPath expected string'); x }
.crucible_query_array <- function(x) { if (!.crucible_query_is_array(x)) .crucible_query_fail('JMESPath expected array'); x }
.crucible_query_object <- function(x) { if (!.crucible_query_is_object(x)) .crucible_query_fail('JMESPath expected object'); x }
.crucible_query_truth <- function(x) {
  if (is.null(x)) return(FALSE)
  if (is.logical(x)) return(isTRUE(x))
  if (.crucible_query_is_string(x)) return(nchar(x) > 0L)
  if (is.list(x)) return(length(x) > 0L)
  TRUE
}
.crucible_query_integer <- function(x) {
  value <- suppressWarnings(as.numeric(as.character(.crucible_query_number(x))))
  if (!is.finite(value) || value != trunc(value) || abs(value) > .Machine$integer.max) .crucible_query_fail('JMESPath integer out of range')
  as.integer(value)
}
# Exact decimal arithmetic on coefficient/exponent pairs.
.crucible_digits_compare <- function(a, b) {
  a <- sub('^0+', '', a); b <- sub('^0+', '', b)
  if (nchar(a) != nchar(b)) return(if (nchar(a) < nchar(b)) -1L else 1L)
  if (a == b) 0L else if (a < b) -1L else 1L
}
.crucible_digits_add <- function(a, b) {
  x <- rev(as.integer(strsplit(a, '', fixed = TRUE)[[1L]])); y <- rev(as.integer(strsplit(b, '', fixed = TRUE)[[1L]]))
  size <- max(length(x), length(y)); x <- c(x, integer(size - length(x))); y <- c(y, integer(size - length(y)))
  out <- integer(size + 1L); carry <- 0L
  for (i in seq_len(size)) { s <- x[i] + y[i] + carry; out[i] <- s %% 10L; carry <- s %/% 10L }
  out[size + 1L] <- carry
  value <- sub('^0+', '', paste0(rev(out), collapse = '')); if (value == '') '0' else value
}
# Requires a >= b.
.crucible_digits_subtract <- function(a, b) {
  # Alignment can pad zero to more digits than the larger coefficient.
  # Compare and subtract must use the same significant digit widths.
  a <- sub('^0+', '', a); b <- sub('^0+', '', b)
  if (a == '') a <- '0'
  if (b == '') b <- '0'
  x <- rev(as.integer(strsplit(a, '', fixed = TRUE)[[1L]])); y <- rev(as.integer(strsplit(b, '', fixed = TRUE)[[1L]]))
  y <- c(y, integer(length(x) - length(y))); out <- integer(length(x)); borrow <- 0L
  for (i in seq_along(x)) { s <- x[i] - y[i] - borrow; borrow <- if (s < 0L) 1L else 0L; out[i] <- s + 10L * borrow }
  value <- sub('^0+', '', paste0(rev(out), collapse = '')); if (value == '') '0' else value
}
.crucible_decimal <- function(x) {
  parts <- .crucible_canonical_number(.crucible_query_number(x))
  exponent <- as.numeric(parts[2L])
  if (!is.finite(exponent) || abs(exponent) > 1e6) .crucible_query_fail('JMESPath arithmetic exponent exceeds supported precision')
  list(negative = startsWith(parts[1L], '-'), digits = sub('^-', '', parts[1L]), exponent = exponent)
}
.crucible_decimal_text <- function(negative, digits, exponent) {
  digits <- sub('^0+', '', digits); if (digits == '') return(json_number('0'))
  if (exponent >= 0) text <- paste0(digits, strrep('0', exponent)) else {
    digits <- paste0(strrep('0', max(0, -exponent - nchar(digits) + 1)), digits)
    point <- nchar(digits) + exponent
    text <- sub('[.]?0*$', '', paste0(substr(digits, 1L, point), '.', substr(digits, point + 1L, nchar(digits))))
  }
  json_number(paste0(if (negative) '-' else '', text))
}
.crucible_decimal_add <- function(a, b) {
  exponent <- min(a$exponent, b$exponent)
  if (max(a$exponent, b$exponent) - exponent > 1e5) .crucible_query_fail('JMESPath arithmetic exceeds supported precision')
  x <- paste0(a$digits, strrep('0', a$exponent - exponent)); y <- paste0(b$digits, strrep('0', b$exponent - exponent))
  if (a$negative == b$negative) return(list(negative = a$negative, digits = .crucible_digits_add(x, y), exponent = exponent))
  order <- .crucible_digits_compare(x, y)
  if (order == 0L) return(list(negative = FALSE, digits = '0', exponent = 0))
  if (order > 0L) list(negative = a$negative, digits = .crucible_digits_subtract(x, y), exponent = exponent)
  else list(negative = b$negative, digits = .crucible_digits_subtract(y, x), exponent = exponent)
}
.crucible_query_compare <- function(left, right) {
  if (!.crucible_query_is_number(left)) return(.crucible_text_compare(.crucible_query_text(left), .crucible_query_text(right)))
  a <- .crucible_decimal(left); b <- .crucible_decimal(right)
  sign <- function(x) if (x$digits == '0') 0L else if (x$negative) -1L else 1L
  as <- sign(a); bs <- sign(b)
  if (as != bs) return(if (as < bs) -1L else 1L)
  if (as == 0L) return(0L)
  ax <- a$exponent + nchar(a$digits); bx <- b$exponent + nchar(b$digits)
  if (ax != bx) return(as * (if (ax < bx) -1L else 1L))
  count <- max(nchar(a$digits), nchar(b$digits))
  as * .crucible_digits_compare(paste0(a$digits, strrep('0', count - nchar(a$digits))), paste0(b$digits, strrep('0', count - nchar(b$digits))))
}
# Code point order, independent of the session locale.
.crucible_text_compare <- function(a, b) {
  x <- utf8ToInt(a); y <- utf8ToInt(b); n <- min(length(x), length(y))
  for (i in seq_len(n)) if (x[i] != y[i]) return(if (x[i] < y[i]) -1L else 1L)
  if (length(x) == length(y)) 0L else if (length(x) < length(y)) -1L else 1L
}
.crucible_query_equal <- function(left, right) {
  if (is.null(left) || is.null(right)) return(is.null(left) && is.null(right))
  if (.crucible_query_is_number(left) || .crucible_query_is_number(right)) return(.crucible_query_is_number(left) && .crucible_query_is_number(right) && .crucible_query_compare(left, right) == 0L)
  if (.crucible_query_is_bool(left) || .crucible_query_is_bool(right)) return(.crucible_query_is_bool(left) && .crucible_query_is_bool(right) && left == right)
  if (.crucible_query_is_string(left) || .crucible_query_is_string(right)) return(.crucible_query_is_string(left) && .crucible_query_is_string(right) && identical(enc2utf8(left), enc2utf8(right)))
  if (.crucible_query_is_object(left) || .crucible_query_is_object(right)) {
    if (!.crucible_query_is_object(left) || !.crucible_query_is_object(right) || length(left) != length(right) || !setequal(names(left), names(right))) return(FALSE)
    for (key in names(left)) if (!.crucible_query_equal(left[[key]], right[[key]])) return(FALSE)
    return(TRUE)
  }
  if (length(left) != length(right)) return(FALSE)
  for (i in seq_along(left)) if (!.crucible_query_equal(left[[i]], right[[i]])) return(FALSE)
  TRUE
}
.crucible_query_append <- function(list, value) { list[length(list) + 1L] <- list(value); list }
.crucible_query_keys <- function(object) { keys <- names(object); if (is.null(keys)) character() else keys[order(vapply(keys, function(k) paste(sprintf('%08x', utf8ToInt(k)), collapse = ''), ''), method = 'radix')] }
.crucible_query_values <- function(object) unname(lapply(.crucible_query_keys(object), function(key) object[[key]]))
.crucible_query_ordered <- function(values) {
  values <- .crucible_query_array(values)
  if (length(values)) {
    numeric <- .crucible_query_is_number(values[[1L]])
    for (item in values) if (numeric) .crucible_query_number(item) else .crucible_query_text(item)
  }
  .crucible_query_sort(values, values)
}
# Stable insertion of indices by key; inputs are small waiter and endpoint values.
.crucible_query_sort <- function(values, keys) {
  index <- seq_along(values)
  for (i in seq_along(index)[-1L]) {
    j <- i; current <- index[i]
    while (j > 1L && .crucible_query_compare(keys[[index[j - 1L]]], keys[[current]]) > 0L) { index[j] <- index[j - 1L]; j <- j - 1L }
    index[j] <- current
  }
  values[index]
}
.crucible_query <- function(node, input) {
  eval <- function(name, value = input) .crucible_query(node[[name]], value)
  kind <- node[['kind']]
  switch(kind,
    identity = input,
    literal = node[['value']],
    field = if (.crucible_query_is_object(input)) input[[node[['name']]]] else NULL,
    index = {
      if (!.crucible_query_is_array(input)) return(NULL)
      index <- .crucible_query_integer(node[['index']]); if (index < 0L) index <- index + length(input)
      if (index < 0L || index >= length(input)) NULL else input[[index + 1L]]
    },
    subexpression = .crucible_query(node[['right']], eval('left')),
    projection = {
      list <- eval('left'); if (!.crucible_query_is_array(list)) return(NULL)
      result <- list(); for (item in list) { value <- .crucible_query(node[['right']], item); if (!is.null(value)) result <- .crucible_query_append(result, value) }
      result
    },
    flatten = {
      list <- eval('expression'); if (!.crucible_query_is_array(list)) return(NULL)
      result <- list(); for (item in list) if (.crucible_query_is_array(item)) { for (nested in item) result <- .crucible_query_append(result, nested) } else result <- .crucible_query_append(result, item)
      result
    },
    objectValues = { object <- eval('expression'); if (.crucible_query_is_object(object)) .crucible_query_values(object) else NULL },
    multiList = { if (is.null(input)) return(NULL); lapply(node[['expressions']], .crucible_query, input = input) },
    multiHash = {
      if (is.null(input)) return(NULL)
      result <- json_object(); for (entry in node[['entries']]) result[entry[['key']]] <- list(.crucible_query(entry[['value']], input)); result
    },
    and = { left <- eval('left'); if (.crucible_query_truth(left)) eval('right') else left },
    or = { left <- eval('left'); if (.crucible_query_truth(left)) left else eval('right') },
    not = !.crucible_query_truth(eval('expression')),
    condition = if (.crucible_query_truth(eval('predicate'))) eval('then') else NULL,
    compare = {
      left <- eval('left'); right <- eval('right'); comparator <- node[['comparator']]
      if (comparator == 'equal') return(.crucible_query_equal(left, right))
      if (comparator == 'notEqual') return(!.crucible_query_equal(left, right))
      if (!.crucible_query_is_number(left) || !.crucible_query_is_number(right)) return(NULL)
      order <- .crucible_query_compare(left, right)
      switch(comparator, lessThan = order < 0L, lessThanEqual = order <= 0L, greaterThan = order > 0L, greaterThanEqual = order >= 0L, .crucible_query_fail('Unknown JMESPath comparator'))
    },
    slice = {
      if (!.crucible_query_is_array(input)) return(NULL)
      size <- length(input); step <- if (is.null(node[['step']])) 1L else .crucible_query_integer(node[['step']])
      if (step == 0L) .crucible_query_fail('Zero slice step')
      boundary <- function(name, fallback) {
        if (is.null(node[[name]])) return(fallback)
        value <- .crucible_query_integer(node[[name]]); if (value < 0L) value <- value + size
        if (step > 0L) min(size, max(0L, value)) else min(size - 1L, max(-1L, value))
      }
      start <- boundary('start', if (step > 0L) 0L else size - 1L); stop <- boundary('stop', if (step > 0L) size else -1L)
      result <- list(); i <- start
      while ((step > 0L && i < stop) || (step < 0L && i > stop)) { result <- .crucible_query_append(result, input[[i + 1L]]); i <- i + step }
      result
    },
    `function` = .crucible_query_function(node[['function']], node[['arguments']], input),
    .crucible_query_fail('Unknown compiled JMESPath node'))
}
.crucible_query_function <- function(name, nodes, input) {
  variadic <- name %in% c('merge', 'not_null')
  arity <- if (name %in% c('contains', 'starts_with', 'ends_with', 'join', 'map', 'max_by', 'min_by', 'sort_by')) 2L else 1L
  if ((variadic && !length(nodes)) || (!variadic && length(nodes) != arity)) .crucible_query_fail('JMESPath invalid function arity')
  arg <- function(i) .crucible_query(nodes[[i]], input)
  reference <- function(i, value) {
    node <- nodes[[i]]
    if (!identical(node[['kind']], 'expressionReference')) .crucible_query_fail('JMESPath expected expression reference')
    .crucible_query(node[['expression']], value)
  }
  switch(name,
    abs = json_number(sub('^-', '', as.character(.crucible_query_number(arg(1L))))),
    ceil = , floor = {
      value <- .crucible_decimal(arg(1L))
      if (value$exponent >= 0) return(.crucible_decimal_text(value$negative, value$digits, value$exponent))
      keep <- nchar(value$digits) + value$exponent
      whole <- if (keep > 0) substr(value$digits, 1L, keep) else '0'
      fraction <- if (keep > 0) substr(value$digits, keep + 1L, nchar(value$digits)) else value$digits
      if (grepl('[1-9]', fraction) && ((name == 'ceil') != value$negative)) whole <- .crucible_digits_add(whole, '1')
      .crucible_decimal_text(value$negative, whole, 0)
    },
    sum = , avg = {
      list <- .crucible_query_array(arg(1L)); total <- list(negative = FALSE, digits = '0', exponent = 0)
      for (item in list) total <- .crucible_decimal_add(total, .crucible_decimal(item))
      if (name == 'sum') return(.crucible_decimal_text(total$negative, total$digits, total$exponent))
      if (!length(list)) return(NULL)
      # Pad normalized coefficients to 34 fractional digits, retaining existing deeper precision.
      digits <- sub('0+$', '', total$digits)
      exponent <- total$exponent + nchar(total$digits) - nchar(digits)
      pad <- max(0, exponent + 34)
      digits <- paste0(digits, strrep('0', pad)); exponent <- exponent - pad
      source <- as.integer(strsplit(digits, '', fixed = TRUE)[[1L]])
      quotient <- integer(length(source)); remainder <- 0
      for (i in seq_along(source)) { remainder <- remainder * 10 + source[i]; quotient[i] <- remainder %/% length(list); remainder <- remainder %% length(list) }
      .crucible_decimal_text(total$negative, paste0(quotient, collapse = ''), exponent)
    },
    contains = {
      source <- arg(1L); wanted <- arg(2L)
      if (.crucible_query_is_string(source)) return(grepl(.crucible_query_text(wanted), source, fixed = TRUE))
      for (item in .crucible_query_array(source)) if (.crucible_query_equal(item, wanted)) return(TRUE)
      FALSE
    },
    starts_with = startsWith(.crucible_query_text(arg(1L)), .crucible_query_text(arg(2L))),
    ends_with = endsWith(.crucible_query_text(arg(1L)), .crucible_query_text(arg(2L))),
    join = paste(vapply(.crucible_query_array(arg(2L)), .crucible_query_text, ''), collapse = .crucible_query_text(arg(1L))),
    keys = as.list(.crucible_query_keys(.crucible_query_object(arg(1L)))),
    values = .crucible_query_values(.crucible_query_object(arg(1L))),
    length = {
      value <- arg(1L)
      json_number(as.character(if (is.list(value)) length(value) else nchar(.crucible_query_text(value), type = 'chars')))
    },
    map = lapply(.crucible_query_array(arg(2L)), function(item) reference(1L, item)),
    max = , min = , sort = {
      list <- .crucible_query_ordered(arg(1L))
      if (name == 'sort') return(list)
      if (!length(list)) return(NULL)
      if (name == 'max') list[[length(list)]] else list[[1L]]
    },
    max_by = , min_by = , sort_by = {
      list <- .crucible_query_array(arg(1L)); keys <- lapply(list, function(item) reference(2L, item)); .crucible_query_ordered(keys)
      if (!length(list)) return(if (name == 'sort_by') list() else NULL)
      if (name == 'sort_by') return(.crucible_query_sort(list, keys))
      selected <- 1L
      for (i in seq_along(list)[-1L]) { order <- .crucible_query_compare(keys[[i]], keys[[selected]]); if ((name == 'max_by' && order > 0L) || (name == 'min_by' && order < 0L)) selected <- i }
      list[[selected]]
    },
    merge = {
      result <- json_object()
      for (i in seq_along(nodes)) { object <- .crucible_query_object(arg(i)); for (key in names(object)) result[key] <- list(object[[key]]) }
      result
    },
    not_null = { for (i in seq_along(nodes)) { value <- arg(i); if (!is.null(value)) return(value) }; NULL },
    reverse = { value <- arg(1L); if (.crucible_query_is_array(value)) rev(value) else intToUtf8(rev(utf8ToInt(.crucible_query_text(value)))) },
    to_array = { value <- arg(1L); if (.crucible_query_is_array(value)) value else list(value) },
    to_number = {
      value <- arg(1L)
      if (.crucible_query_is_number(value)) return(value)
      if (.crucible_query_is_string(value) && grepl('^-?(0|[1-9][0-9]*)([.][0-9]+)?([eE][+-]?[0-9]+)?$', value)) return(json_number(value))
      NULL
    },
    to_string = { value <- arg(1L); if (.crucible_query_is_string(value)) value else json_encode(value) },
    type = {
      value <- arg(1L)
      if (is.null(value)) return('null')
      if (.crucible_query_is_bool(value)) return('boolean')
      if (.crucible_query_is_number(value)) return('number')
      if (.crucible_query_is_string(value)) return('string')
      if (.crucible_query_is_object(value)) return('object')
      if (.crucible_query_is_array(value)) return('array')
      .crucible_query_fail('Unknown JMESPath value')
    },
    .crucible_query_fail('Unknown compiled JMESPath function'))
}
# Converts codec-encoded wire values (keyed by member name) to the query data model:
# timestamps become exact epoch seconds, blobs stay base64 text.
.crucible_query_value <- function(target, value, format = NULL) {
  if (is.null(target) || is.null(value) || inherits(value, 'crucible_unset')) return(NULL)
  shape <- .crucible_shape(target)
  switch(shape$kind,
    structure = , union = {
      result <- json_object()
      for (name in names(value)) {
        member <- shape$members[[name]]
        result[name] <- list(if (is.null(member)) value[[name]] else .crucible_query_value(member$target, value[[name]], member$format))
      }
      result
    },
    list = lapply(value, .crucible_query_value, target = shape$member$target, format = shape$member$format),
    map = .crucible_object(lapply(value, .crucible_query_value, target = shape$value$target, format = shape$value$format)),
    timestamp = {
      if (is.null(format)) format <- shape$format
      .crucible_time_encode(.crucible_time_decode(value, format), 'epoch-seconds')
    },
    value)
}
