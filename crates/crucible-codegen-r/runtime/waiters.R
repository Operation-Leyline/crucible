# Modeled waiter acceptors and timing.
.crucible_waiter_condition <- function(timeout, name, attempts, response, error, message = NULL) {
  text <- paste0('Waiter ', name, if (timeout) ' timed out' else ' failed', ' after ', attempts, ' attempts')
  if (!is.null(message)) text <- paste0(text, ': ', paste(unlist(message), collapse = ', '))
  .crucible_condition(c(if (timeout) 'crucible_waiter_timeout', 'crucible_waiter_error'), text,
    waiter = name, attempts = attempts, response = response, error = error, reason = message)
}
# Returns list(state, message). A query failure is an error for the caller.
.crucible_waiter_state <- function(description, input, output, error) {
  for (acceptor in description[['acceptors']]) {
    matcher <- acceptor[['matcher']]; kind <- matcher[['kind']]; match <- FALSE
    if (kind == 'success') match <- is.null(error) == isTRUE(matcher[['expected']])
    else if (kind == 'errorType') {
      name <- if (inherits(error, 'crucible_service_error')) error$metadata$name else NULL
      expected <- matcher[['name']]
      match <- !is.null(name) && name %in% c(expected, sub('^.*#', '', expected))
    } else if (is.null(error)) {
      result <- .crucible_query(matcher[['path']][['expression']], if (kind == 'inputOutput') json_object(input = input, output = output) else output)
      expected <- matcher[['expected']]
      strings <- function(value) .crucible_query_is_array(value) && length(value) > 0L
      match <- switch(matcher[['comparator']],
        stringEquals = .crucible_query_is_string(result) && result == expected,
        booleanEquals = .crucible_query_is_bool(result) && result == identical(expected, 'true'),
        allStringEquals = strings(result) && all(vapply(result, function(item) .crucible_query_is_string(item) && item == expected, TRUE)),
        anyStringEquals = .crucible_query_is_array(result) && any(vapply(result, function(item) .crucible_query_is_string(item) && item == expected, TRUE)),
        FALSE)
    }
    if (isTRUE(match)) {
      message <- NULL
      if (!is.null(acceptor[['message']])) {
        value <- .crucible_query(acceptor[['message']][['expression']], output)
        if (.crucible_query_is_string(value) || (.crucible_query_is_array(value) && all(vapply(value, .crucible_query_is_string, TRUE)))) message <- value
      }
      return(list(state = acceptor[['state']], message = message))
    }
  }
  list(state = if (is.null(error)) 'retry' else 'failure', message = NULL)
}
# Jittered backoff reserves a minimum call budget for the final poll.
.crucible_waiter_delay <- function(minimum, maximum, attempt, remaining, uniform = function(low, high) stats::runif(1L, low, high)) {
  ceiling <- min(maximum, minimum * 2^min(attempt - 1, 62))
  delay <- if (ceiling > minimum) uniform(minimum, ceiling) else minimum
  if (remaining <= minimum) remaining else if (remaining - delay <= minimum) remaining - minimum else delay
}
