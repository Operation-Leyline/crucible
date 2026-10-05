package sdk

import (
    "bytes"
    "crypto/md5"
    "crypto/sha1"
    "crypto/sha256"
    "crypto/sha512"
    "crypto/subtle"
    "encoding/base64"
    "encoding/binary"
    "fmt"
    "hash/crc32"
    "hash/crc64"
    "io"
    "net/http"
    "strings"
)

type ChecksumPolicy string
const (
    ChecksumWhenSupported ChecksumPolicy = "WHEN_SUPPORTED"
    ChecksumWhenRequired ChecksumPolicy = "WHEN_REQUIRED"
)

type ChecksumError struct { Algorithm string }
func (e *ChecksumError) Error() string { return "response " + e.Algorithm + " checksum mismatch" }

type checksumDescription struct {
    Kind string
    RequestRequired bool
    RequestAlgorithmMember string
    ValidationModeMember string
    ResponseAlgorithms []string
}
type checksumSettings struct { Algorithm, Header string; Response []string }
var crc32cTable = crc32.MakeTable(crc32.Castagnoli)
var crc64nvmeTable = crc64.MakeTable(0x9a6c9329ac4bc9b5)

func checksumSupported(algorithm string) bool {
    switch algorithm { case "CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512": return true }; return false
}
func checksumDigest(algorithm string, data []byte) (string, error) {
    var raw []byte
    switch algorithm {
    case "CRC32", "CRC32C":
        table := crc32.IEEETable; if algorithm == "CRC32C" { table = crc32cTable }
        raw = make([]byte,4); binary.BigEndian.PutUint32(raw,crc32.Checksum(data,table))
    case "CRC64NVME": raw = make([]byte,8); binary.BigEndian.PutUint64(raw,crc64.Checksum(data,crc64nvmeTable))
    case "MD5": value := md5.Sum(data); raw = value[:]
    case "SHA1": value := sha1.Sum(data); raw = value[:]
    case "SHA256": value := sha256.Sum256(data); raw = value[:]
    case "SHA512": value := sha512.Sum512(data); raw = value[:]
    default: return "",fmt.Errorf("unsupported checksum algorithm: %s",algorithm)
    }
    return base64.StdEncoding.EncodeToString(raw),nil
}
func prepareChecksum(description *checksumDescription, shape shapeDescription, values map[string]any, requestPolicy,responsePolicy ChecksumPolicy) (checksumSettings,error) {
    settings := checksumSettings{}
    if description == nil { return settings,nil }
    if description.Kind == "md5" { settings.Algorithm,settings.Header="MD5","Content-MD5"; return settings,nil }
    if member := description.RequestAlgorithmMember; member != "" {
        wire := shape.wireName(member)
        algorithm,_ := values[wire].(string)
        if algorithm == "" && (description.RequestRequired || requestPolicy != ChecksumWhenRequired) { algorithm="CRC32"; values[wire]=algorithm }
        if algorithm != "" {
            if !checksumSupported(algorithm) { return settings,fmt.Errorf("unsupported checksum algorithm: %s",algorithm) }
            settings.Algorithm,settings.Header=algorithm,"X-Amz-Checksum-"+strings.ToLower(algorithm)
        }
    } else if description.RequestRequired { settings.Algorithm,settings.Header="MD5","Content-MD5" }
    if member := description.ValidationModeMember; member != "" {
        wire := shape.wireName(member); mode,_ := values[wire].(string)
        if mode == "" && responsePolicy != ChecksumWhenRequired { mode="ENABLED"; values[wire]=mode }
        if mode == "ENABLED" { for _,algorithm := range description.ResponseAlgorithms { if checksumSupported(algorithm) { settings.Response=append(settings.Response,algorithm) } } }
    }
    return settings,nil
}
func (s checksumSettings) request(request *http.Request) error {
    if s.Header == "" || request.Header.Get(s.Header) != "" { return nil }
    var data []byte; var err error
    if request.Body != nil { data,err=io.ReadAll(request.Body); request.Body.Close(); if err!=nil{return err} }
    request.Body=io.NopCloser(bytes.NewReader(data)); request.ContentLength=int64(len(data))
    request.GetBody=func()(io.ReadCloser,error){return io.NopCloser(bytes.NewReader(data)),nil}
    value,err := checksumDigest(s.Algorithm,data); if err!=nil{return err}; request.Header.Set(s.Header,value); return nil
}
func (s checksumSettings) response(headers http.Header,data []byte)(string,error){
    for _,algorithm:=range s.Response {
        expected:=headers.Get("X-Amz-Checksum-"+strings.ToLower(algorithm)); if expected==""{continue}
        actual,err:=checksumDigest(algorithm,data);if err!=nil{return "",err}
        if subtle.ConstantTimeCompare([]byte(expected),[]byte(actual))!=1{return "",&ChecksumError{Algorithm:algorithm}}
        return algorithm,nil
    }
    return "",nil
}
