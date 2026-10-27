args <- commandArgs(TRUE)
library(docs.client, lib.loc = args[1L])
cases <- json_parse(readChar(Sys.getenv('ERROR_DOCS_CASES'), file.info(Sys.getenv('ERROR_DOCS_CASES'))$size))$cases
for (test in cases) {
  client <- TroubleshootingClient(client_config(endpoint = Sys.getenv('ERROR_DOCS_URL'), max_attempts = 1L))
  error <- tryCatch({ client$inspect(); NULL }, TroubleshootingException = identity)
  client$close()
  rm(client)
  invisible(gc())
  if (!inherits(error, 'TroubleshootingException')) stop(test$name, ': missing service condition')
  if (!identical(error$metadata$doc_url, test$docUrl)) stop(test$name, ': wrong documentation URL')
  if (!is.null(test$typed) && !identical(!is.null(error$model), test$typed)) stop(test$name, ': wrong modeled classification')
  if (test$name == 'default-known') stopifnot(as.character(error$model$detail) == 'access/needed')
  if (test$name == 'default-empty') stopifnot(as.character(error$model$detail) == '')
  if (test$name == 'mapped-member') stopifnot(as.character(error$model$failure_reason) == 'access/needed')
  if (test$name %in% c('document-empty', 'document-whitespace', 'document-null')) {
    stopifnot(inherits(error$model$data, 'crucible_unset') == (test$name != 'document-null'))
    if (test$name == 'document-null') stopifnot(is.null(error$model$data))
  }
}
cat('R installed error-documentation HTTP and condition lifetime passed\n')
