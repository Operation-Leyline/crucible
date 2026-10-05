#include <R.h>
#include <Rinternals.h>
#include <stdint.h>

/* Reflected table CRCs. The package already compiles one native routine, and a
 * byte loop interpreted by R is orders of magnitude slower for request bodies. */
static uint32_t crc32_table[2][256];
static uint64_t crc64_table[256];
static int tables_ready = 0;

static void build_tables(void) {
    const uint32_t polynomials[2] = {0xEDB88320u, 0x82F63B78u};
    for (int kind = 0; kind < 2; kind++) {
        for (uint32_t byte = 0; byte < 256; byte++) {
            uint32_t value = byte;
            for (int bit = 0; bit < 8; bit++) value = (value >> 1) ^ ((value & 1u) ? polynomials[kind] : 0u);
            crc32_table[kind][byte] = value;
        }
    }
    for (uint64_t byte = 0; byte < 256; byte++) {
        uint64_t value = byte;
        for (int bit = 0; bit < 8; bit++) value = (value >> 1) ^ ((value & 1u) ? UINT64_C(0x9A6C9329AC4BC9B5) : 0u);
        crc64_table[byte] = value;
    }
    tables_ready = 1;
}

/* kind: 0 = CRC32, 1 = CRC32C, 2 = CRC64NVME. Returns big-endian raw bytes. */
SEXP crucible_crc(SEXP data, SEXP kind) {
    if (TYPEOF(data) != RAWSXP || TYPEOF(kind) != INTSXP || XLENGTH(kind) != 1)
        Rf_error("CRC requires a raw vector and an integer kind");
    int algorithm = INTEGER(kind)[0];
    if (algorithm < 0 || algorithm > 2) Rf_error("Unknown CRC kind");
    if (!tables_ready) build_tables();
    const unsigned char *bytes = RAW(data);
    R_xlen_t size = XLENGTH(data);
    if (algorithm == 2) {
        uint64_t value = ~UINT64_C(0);
        for (R_xlen_t i = 0; i < size; i++) value = crc64_table[(value ^ bytes[i]) & 0xFFu] ^ (value >> 8);
        value = ~value;
        SEXP result = PROTECT(Rf_allocVector(RAWSXP, 8));
        for (int i = 0; i < 8; i++) RAW(result)[i] = (Rbyte) (value >> (56 - 8 * i));
        UNPROTECT(1);
        return result;
    }
    uint32_t value = ~UINT32_C(0);
    for (R_xlen_t i = 0; i < size; i++) value = crc32_table[algorithm][(value ^ bytes[i]) & 0xFFu] ^ (value >> 8);
    value = ~value;
    SEXP result = PROTECT(Rf_allocVector(RAWSXP, 4));
    for (int i = 0; i < 4; i++) RAW(result)[i] = (Rbyte) (value >> (24 - 8 * i));
    UNPROTECT(1);
    return result;
}
