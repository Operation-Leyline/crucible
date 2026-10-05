# Evaluates the shared endpoint rules IR; the Smithy traits are never read here.
.crucible_endpoint_fail <- function(message) stop(.crucible_condition('crucible_endpoint_error', message))
.crucible_host_label <- function(value, subdomains = FALSE) {
  if (!.crucible_query_is_string(value) || value == '') return(FALSE)
  labels <- if (subdomains) strsplit(paste0(value, '.sentinel'), '.', fixed = TRUE)[[1L]] else value
  if (subdomains) labels <- head(labels, -1L)
  length(labels) > 0L && all(grepl('^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?$', labels))
}
.crucible_ip <- function(host) {
  if (grepl('^[0-9]{1,3}([.][0-9]{1,3}){3}$', host)) return(all(as.integer(strsplit(host, '.', fixed = TRUE)[[1L]]) <= 255L))
  if (!grepl(':', host, fixed = TRUE) || !grepl('^[0-9A-Fa-f:.]+$', host) || lengths(regmatches(host, gregexpr('::', host, fixed = TRUE))) > 1L) return(FALSE)
  groups <- strsplit(host, ':', fixed = TRUE)[[1L]]
  all(grepl('^[0-9A-Fa-f]{0,4}$', groups[!grepl('.', groups, fixed = TRUE)])) && length(groups) <= 8L
}
.crucible_parse_url <- function(value) {
  if (!.crucible_query_is_string(value) || grepl('[[:cntrl:] ]', value) || grepl('[?#]', value)) return(NULL)
  m <- regmatches(value, regexec('^([A-Za-z][A-Za-z0-9+.-]*)://([^/]*)(/.*)?$', value))[[1L]]
  if (!length(m)) return(NULL)
  scheme <- tolower(m[2L]); authority <- m[3L]; path <- if (m[4L] == '') '/' else m[4L]
  if (!scheme %in% c('http', 'https') || grepl('@', authority, fixed = TRUE)) return(NULL)
  parts <- if (startsWith(authority, '[')) regmatches(authority, regexec('^\\[([^]]+)\\](:([0-9]*))?$', authority))[[1L]] else regmatches(authority, regexec('^([^:]+)(:([0-9]*))?$', authority))[[1L]]
  if (!length(parts)) return(NULL)
  host <- tolower(parts[2L]); port <- parts[4L]
  if (port != '' && (nchar(port) > 5L || as.integer(port) > 65535L)) return(NULL)
  is_ip <- .crucible_ip(host)
  if (startsWith(authority, '[') && !is_ip) return(NULL)
  if (!is_ip && !.crucible_host_label(sub('[.]$', '', host), TRUE)) return(NULL)
  authority <- if (grepl(':', host, fixed = TRUE)) paste0('[', host, ']') else host
  if (port != '' && as.integer(port) != (if (scheme == 'https') 443L else 80L)) authority <- paste0(authority, ':', as.integer(port))
  json_object(scheme = scheme, authority = authority, path = path, normalizedPath = if (endsWith(path, '/')) path else paste0(path, '/'), isIp = is_ip, host = host, port = port)
}
.crucible_endpoint_attribute <- function(value, path) {
  steps <- path
  if (.crucible_query_is_string(path)) {
    steps <- list()
    for (part in strsplit(path, '.', fixed = TRUE)[[1L]]) {
      m <- regmatches(part, regexec('^([^[]*)(\\[(-?[0-9]+)\\])?$', part))[[1L]]
      if (!length(m)) .crucible_endpoint_fail('Invalid endpoint attribute path')
      if (m[2L] != '') steps[[length(steps) + 1L]] <- list(kind = 'member', name = m[2L])
      if (m[3L] != '') steps[[length(steps) + 1L]] <- list(kind = 'index', index = as.integer(m[4L]))
    }
  }
  for (step in steps) {
    if (identical(step[['kind']], 'member')) {
      value <- if (.crucible_query_is_object(value)) value[[step[['name']]]] else NULL
    } else {
      position <- as.integer(as.character(step[['index']]))
      value <- if (.crucible_query_is_array(value) && position >= -length(value) && position < length(value)) value[[if (position < 0L) length(value) + position + 1L else position + 1L]] else NULL
    }
    if (is.null(value)) break
  }
  value
}
.crucible_endpoint_function <- function(name, args) {
  switch(name,
    isSet = !is.null(args[[1L]]),
    not = { if (!.crucible_query_is_bool(args[[1L]])) .crucible_endpoint_fail('Endpoint not expects a boolean'); !args[[1L]] },
    booleanEquals = length(args) == 2L && .crucible_query_is_bool(args[[1L]]) && .crucible_query_is_bool(args[[2L]]) && args[[1L]] == args[[2L]],
    stringEquals = length(args) == 2L && .crucible_query_is_string(args[[1L]]) && .crucible_query_is_string(args[[2L]]) && identical(enc2utf8(args[[1L]]), enc2utf8(args[[2L]])),
    getAttr = .crucible_endpoint_attribute(args[[1L]], args[[2L]]),
    substring = {
      text <- args[[1L]]; start <- as.integer(as.character(args[[2L]])); end <- as.integer(as.character(args[[3L]])); reverse <- isTRUE(args[[4L]])
      if (!.crucible_query_is_string(text) || any(utf8ToInt(text) > 127L) || start < 0L || start > end || end > nchar(text)) return(NULL)
      if (reverse) substr(text, nchar(text) - end + 1L, nchar(text) - start) else substr(text, start + 1L, end)
    },
    isValidHostLabel = .crucible_host_label(args[[1L]], isTRUE(args[[2L]])),
    parseUrl = { url <- .crucible_parse_url(args[[1L]]); if (is.null(url)) NULL else url[c('scheme', 'authority', 'path', 'normalizedPath', 'isIp')] },
    uriEncode = curl::curl_escape(.crucible_query_text(args[[1L]])),
    .crucible_endpoint_fail(paste('Unsupported compiled endpoint function', name)))
}
.crucible_endpoint_expression <- function(node, values) {
  switch(node[['kind']],
    reference = values[[node[['name']]]],
    boolean = , integer = node[['value']],
    array = lapply(node[['values']], .crucible_endpoint_expression, values = values),
    object = { result <- json_object(); for (key in names(node[['values']])) result[key] <- list(.crucible_endpoint_expression(node[['values']][[key]], values)); result },
    call = .crucible_endpoint_function(node[['function']], lapply(node[['arguments']], .crucible_endpoint_expression, values = values)),
    string = paste0(vapply(node[['parts']], function(part) {
      value <- if (identical(part[['kind']], 'literal')) part[['value']] else .crucible_endpoint_attribute(values[[part[['name']]]], part[['path']])
      if (.crucible_query_is_bool(value)) return(if (value) 'true' else 'false')
      if (is.null(value) || !(.crucible_query_is_string(value) || .crucible_query_is_number(value))) .crucible_endpoint_fail('Endpoint template references a missing or non-scalar parameter')
      as.character(value)
    }, ''), collapse = ''),
    .crucible_endpoint_fail(paste('Unsupported compiled endpoint expression', node[['kind']])))
}
.crucible_endpoint_rules <- function(nodes, parameters) {
  for (rule in nodes) {
    scope <- parameters; matched <- TRUE
    for (condition in rule[['conditions']]) {
      value <- .crucible_endpoint_function(condition[['function']], lapply(condition[['arguments']], .crucible_endpoint_expression, values = scope))
      if (is.null(value) || identical(value, FALSE)) { matched <- FALSE; break }
      if (!is.null(condition[['assign']])) scope[condition[['assign']]] <- list(value)
    }
    if (!matched) next
    result <- rule[['result']]
    if (identical(result[['kind']], 'tree')) { nested <- .crucible_endpoint_rules(result[['rules']], scope); if (!is.null(nested)) return(nested); next }
    if (identical(result[['kind']], 'error')) .crucible_endpoint_fail(.crucible_endpoint_expression(result[['message']], scope))
    url <- .crucible_endpoint_expression(result[['url']], scope)
    if (is.null(.crucible_parse_url(url))) .crucible_endpoint_fail('Endpoint rule returned an invalid HTTP(S) URL')
    headers <- list(); for (key in names(result[['headers']])) headers[[key]] <- lapply(result[['headers']][[key]], .crucible_endpoint_expression, values = scope)
    properties <- json_object(); for (key in names(result[['properties']])) properties[key] <- list(.crucible_endpoint_expression(result[['properties']][[key]], scope))
    return(list(url = url, headers = headers, properties = properties))
  }
  NULL
}
.crucible_resolve_endpoint <- function(rule_set, sources, input, client, builtins) {
  parameters <- list()
  for (name in names(rule_set[['parameters']])) {
    parameter <- rule_set[['parameters']][[name]]; value <- NULL
    for (source in sources[[name]]) {
      value <- switch(source[['kind']],
        static = , default = source[['value']],
        inputMember = if (.crucible_query_is_object(input)) input[[source[['member']]]] else NULL,
        inputPath = .crucible_query(source[['path']][['expression']], input),
        client = client[[source[['name']]]],
        builtIn = builtins[[source[['name']]]],
        NULL)
      if (!is.null(value)) break
    }
    if (is.null(value) && isTRUE(parameter[['required']])) .crucible_endpoint_fail(paste('Required endpoint parameter', name, 'is missing'))
    if (!is.null(value)) {
      valid <- switch(parameter[['kind']], string = .crucible_query_is_string(value), boolean = .crucible_query_is_bool(value),
        .crucible_query_is_array(value) && all(vapply(value, .crucible_query_is_string, TRUE)))
      if (!valid) .crucible_endpoint_fail(paste('Endpoint parameter', name, 'has the wrong type'))
      parameters[name] <- list(value)
    }
  }
  result <- .crucible_endpoint_rules(rule_set[['rules']], parameters)
  if (is.null(result)) .crucible_endpoint_fail('No endpoint rule matched')
  result
}
.crucible_host_prefix <- function(url, description, input) {
  prefix <- ''
  for (part in description[['parts']]) {
    if (identical(part[['kind']], 'literal')) { prefix <- paste0(prefix, part[['value']]); next }
    value <- if (.crucible_query_is_object(input)) input[[part[['member']]]] else NULL
    if (!.crucible_host_label(value)) .crucible_endpoint_fail(paste('Invalid endpoint host label', part[['member']]))
    prefix <- paste0(prefix, value)
  }
  parsed <- .crucible_parse_url(url)
  if (is.null(parsed)) .crucible_endpoint_fail('Invalid endpoint URL')
  host <- paste0(prefix, parsed$host)
  if (nchar(host) > 253L || !.crucible_host_label(host, TRUE)) .crucible_endpoint_fail('Invalid endpoint hostname after prefix expansion')
  paste0(parsed$scheme, '://', host, if (parsed$port != '') paste0(':', parsed$port) else '', if (identical(parsed$path, '/') && !endsWith(url, '/')) '' else parsed$path)
}
