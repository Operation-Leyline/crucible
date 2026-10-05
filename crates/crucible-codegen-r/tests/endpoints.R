args <- commandArgs(trailingOnly = TRUE)
# Load lazily used namespaces up front so call deadlines measure calls, not package loading.
invisible(lapply(c('curl', 'openssl', 'jsonlite', 'stats'), loadNamespace))
suppressPackageStartupMessages(library(behavior.client, lib.loc = args[1L]))
ns <- asNamespace('behavior.client')
internal <- function(name) get(paste0('.crucible_', name), ns)
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)
rejects <- function(expression, class = 'error', pattern = NULL) {
  result <- tryCatch({ force(expression); NULL }, error = identity)
  if (is.null(result) || !inherits(result, class)) stop('Expected ', class, ', got ', paste(format(result), collapse = ' '))
  if (!is.null(pattern) && !grepl(pattern, conditionMessage(result), fixed = TRUE)) stop('Unexpected error: ', conditionMessage(result))
  invisible(result)
}
sent <- list()
transport <- function(request, read_body, scope) { sent[[length(sent) + 1L]] <<- request; list(status = 200L, headers = list(), body = charToRaw('{"wire-status":"READY"}')) }
credentials <- aws_credentials('key', 'secret')
sdk <- function(...) ServiceClient(client_config(transport = transport, credentials = credentials, ...))

# Rules choose the URL, headers, and signing scope; the host prefix uses the input label.
client <- sdk(region = 'local')
check(client$read(Input(account = 'customer'))$status == 'READY', 'Resolved endpoint call')
request <- sent[[length(sent)]]
check(request$url == 'https://customer.local.example.test/resource', 'Rule URL with host prefix')
check(request$headers[['x-region']] == 'local', 'Endpoint header')
check(grepl('/local/example/aws4_request', request$headers$authorization, fixed = TRUE), 'Endpoint auth scheme signing scope')
check(request$headers[['x-amz-content-sha256']] == 'UNSIGNED-PAYLOAD', 'Unsigned payload')
check(client$wait_until_ready(Input(account = 'customer'), timeout = 30)$status == 'READY', 'Waiter on a resolved endpoint')
count <- length(sent)
for (value in c('a.b', '-bad', 'x/y', 'x@evil', '', strrep('x', 64L))) rejects(client$read(Input(account = value)), pattern = 'host label')
check(length(sent) == count, 'Invalid host labels fail before sending')
invisible(sdk(region = 'local', disable_host_prefix = TRUE)$read(Input(account = 'ignored')))
check(sent[[length(sent)]]$url == 'https://local.example.test/resource', 'Disabled host prefix')
rejects(sdk(region = 'missing')$read(Input(account = 'customer')), 'crucible_endpoint_error', 'Unknown region missing')
rejects(sdk()$read(Input(account = 'customer')), 'crucible_endpoint_error', 'missing')
invisible(sdk(region = 'local')$public()); check(is.null(sent[[length(sent)]]$headers$authorization), 'Anonymous operation on resolved endpoint')
rejects(sdk(region = 'local', endpoint_builtins = list('AWS::Region' = 'missing'))$public(), 'crucible_endpoint_error', 'Unknown region missing')

# Parameter sources take the first available value; false is a value.
resolve <- internal('resolve_endpoint')
rules <- json_parse('{"parameters":{"Name":{"kind":"string","required":true},"Flag":{"kind":"boolean","required":true}},
  "rules":[{"conditions":[{"function":"isSet","arguments":[{"kind":"reference","name":"Flag"}],"assign":null}],
  "result":{"kind":"endpoint","url":{"kind":"string","parts":[{"kind":"literal","value":"https://example.test/base"}]},
  "headers":{"name":[{"kind":"reference","name":"Name"}]},"properties":{"flag":{"kind":"reference","name":"Flag"}}}}]}')
sources <- json_parse('{"Name":[{"kind":"inputMember","member":"name"},{"kind":"client","name":"name"},{"kind":"default","value":"default"}],"Flag":[{"kind":"builtIn","name":"Flag"}]}')
result <- resolve(rules, sources, json_object(name = 'input'), list(name = 'client'), list(Flag = FALSE))
check(identical(result$headers$name, list('input')) && identical(result$properties$flag, FALSE), 'Input member precedence and false')
check(identical(resolve(rules, sources, json_object(), list(name = 'client'), list(Flag = FALSE))$headers$name, list('client')), 'Client parameter')
check(identical(resolve(rules, sources, json_object(), list(), list(Flag = FALSE))$headers$name, list('default')), 'Default parameter')
rejects(resolve(rules, sources, json_object(), list(), list()), 'crucible_endpoint_error', 'missing')
rejects(resolve(rules, sources, json_object(), list(), list(Flag = 0)), 'crucible_endpoint_error', 'wrong type')
tree <- json_parse('{"parameters":{},"rules":[{"conditions":[{"function":"parseUrl","arguments":[{"kind":"string","parts":[{"kind":"literal","value":"https://example.test:8443/a"}]}],"assign":"url"}],
  "result":{"kind":"tree","rules":[{"conditions":[],"result":{"kind":"endpoint","url":{"kind":"string","parts":[{"kind":"literal","value":"https://"},{"kind":"reference","name":"url","path":"authority"},{"kind":"literal","value":"/v"}]},"headers":{},"properties":{}}}]}}]}')
check(resolve(tree, list(), NULL, list(), list())$url == 'https://example.test:8443/v', 'Assigned parseUrl attribute in a tree rule')

# Standard library functions and URL validation.
parse <- internal('parse_url'); fn <- internal('endpoint_function')
check(parse('https://example.test:443/a')$authority == 'example.test', 'Default port omitted')
check(parse('https://[::1]:8443')$authority == '[::1]:8443' && isTRUE(parse('https://[::1]')$isIp), 'IPv6 endpoint')
check(isTRUE(parse('http://10.0.0.1/p')$isIp) && parse('http://10.0.0.1/p')$normalizedPath == '/p/', 'IPv4 endpoint')
for (value in c('file:///path', 'https://user:password@example.test', 'https://example.test?q=x', 'https://example.test#x', 'https://example.test:99999', 'https://evil\n.example')) check(is.null(parse(value)), paste('Rejected URL', value))
check(fn('substring', list('abcdef', 1L, 3L, TRUE)) == 'de' && fn('substring', list('abcdef', 1L, 3L, FALSE)) == 'bc', 'substring')
check(is.null(fn('substring', list('é', 0L, 1L, FALSE))) && is.null(fn('substring', list('a', 0L, 2L, FALSE))), 'substring bounds')
check(fn('uriEncode', list('a/b +é')) == 'a%2Fb%20%2B%C3%A9', 'uriEncode')
check(fn('getAttr', list(json_object(items = list('a', 'b')), 'items[1]')) == 'b', 'getAttr')
check(isTRUE(fn('isSet', list(FALSE))) && !fn('isValidHostLabel', list('x.y', FALSE)) && fn('isValidHostLabel', list('x.y', TRUE)), 'isSet and host labels')
check(isTRUE(fn('booleanEquals', list(FALSE, FALSE))) && !fn('stringEquals', list('a', TRUE)), 'Typed equality')
rejects(fn('not', list('x')), 'crucible_endpoint_error')
cat('R endpoint rules, host prefixes, and endpoint auth passed\n')
