.crucible_checksum_algorithms <- c('CRC32', 'CRC32C', 'CRC64NVME', 'MD5', 'SHA1', 'SHA256', 'SHA512')
# Base64 of the checksum over the exact transmitted (or received) bytes.
.crucible_checksum <- function(algorithm, data) {
  if (is.null(data)) data <- raw()
  if (!is.raw(data)) stop('Checksum input must be raw', call. = FALSE)
  digest <- switch(algorithm,
    CRC32 = .Call(C_crucible_crc, data, 0L), CRC32C = .Call(C_crucible_crc, data, 1L), CRC64NVME = .Call(C_crucible_crc, data, 2L),
    MD5 = openssl::md5(data), SHA1 = openssl::sha1(data), SHA256 = openssl::sha256(data), SHA512 = openssl::sha512(data),
    stop('Unsupported checksum algorithm: ', algorithm, call. = FALSE))
  as.character(openssl::base64_encode(as.raw(digest)))
}
# Resolves the operation's checksum settings and the members a policy injects.
# Injected members are wire values bound like caller input; the caller's model is not changed.
.crucible_checksum_prepare <- function(description, values, request_policy, response_policy) {
  if (is.null(description)) return(list(settings = NULL, values = values))
  if (identical(description[['kind']], 'md5')) return(list(settings = list(algorithm = 'MD5', header = 'content-md5', response = character()), values = values))
  algorithm <- NULL; header <- NULL
  member <- description[['requestAlgorithmMember']]
  if (!is.null(member)) {
    algorithm <- values[[member]]
    if (is.null(algorithm) && (isTRUE(description[['requestRequired']]) || request_policy == 'WHEN_SUPPORTED')) { algorithm <- 'CRC32'; values[[member]] <- algorithm }
    if (!is.null(algorithm)) {
      algorithm <- as.character(algorithm)
      if (!algorithm %in% .crucible_checksum_algorithms) stop('Unsupported checksum algorithm: ', algorithm, call. = FALSE)
      header <- paste0('x-amz-checksum-', tolower(algorithm))
    }
  } else if (isTRUE(description[['requestRequired']])) { algorithm <- 'MD5'; header <- 'content-md5' }
  validate <- FALSE; mode <- description[['validationModeMember']]
  if (!is.null(mode)) {
    choice <- values[[mode]]
    if (is.null(choice) && response_policy == 'WHEN_SUPPORTED') { choice <- 'ENABLED'; values[[mode]] <- choice }
    validate <- identical(as.character(choice), 'ENABLED')
  }
  response <- if (validate) intersect(unlist(description[['responseAlgorithms']]), .crucible_checksum_algorithms) else character()
  list(settings = list(algorithm = algorithm, header = header, response = response), values = values)
}
# A header the caller bound or a middleware set is transmitted unchanged.
.crucible_checksum_request <- function(settings, request) {
  header <- settings[['header']]
  if (!is.null(header) && is.null(request$headers[[header]])) request$headers[[header]] <- .crucible_checksum(settings[['algorithm']], request$body)
  request
}
.crucible_checksum_response <- function(settings, headers, body) {
  for (algorithm in settings[['response']]) {
    expected <- headers[[paste0('x-amz-checksum-', tolower(algorithm))]]
    if (!is.null(expected)) {
      if (!identical(expected, .crucible_checksum(algorithm, body))) stop(.crucible_condition('crucible_checksum_error', paste('Response', algorithm, 'checksum mismatch'), algorithm = algorithm))
      return(algorithm)
    }
  }
  NULL
}
