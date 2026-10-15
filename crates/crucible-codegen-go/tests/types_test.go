package types

import (
    "context"
    "encoding/json"
    "io"
    "math/big"
    "net/http"
    "net/http/httptest"
    "reflect"
    "strings"
    "testing"
)

func TestExtendedTypesOverHTTP(t *testing.T) {
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        io.WriteString(w, `{"mode":42,"names":["a","b"],"sparseNames":["a",null],"sparseMap":{"known":-1,"future":42,"absent":null}}`)
    }))
    defer server.Close()
    client, err := NewTypesClient(Config{Endpoint:server.URL})
    if err != nil { t.Fatal(err) }
    output, err := client.Read(context.Background(), Unit{})
    if err != nil { t.Fatal(err) }
    if output.Integer.String() != "12345678901234567890123456789012345678901234567890" { t.Fatal(output.Integer) }
    if output.Decimal.String() != "12345678901234567890.12345678901234567890123456789" { t.Fatal(output.Decimal) }
    if output.GetMode() != Mode(42) || *output.GetSparseNames()[0] != "a" || output.GetSparseNames()[1] != nil { t.Fatal(output) }
    if output.GetSparseMap()["absent"] != nil || *output.GetSparseMap()["known"] != ModeOFF { t.Fatal(output) }
    encoded, err := encodeValue(client.schema, "example.types#Output", reflect.ValueOf(output), "")
    if err != nil { t.Fatal(err) }
    data, err := json.Marshal(encoded)
    if err != nil { t.Fatal(err) }
    if !strings.Contains(string(data), `"integer":12345678901234567890123456789012345678901234567890`) || !strings.Contains(string(data), `"decimal":12345678901234567890.12345678901234567890123456789`) { t.Fatal(string(data)) }
}

func TestExtendedModelValidation(t *testing.T) {
    if !Decimal("10e99999999999999999999999999").Equal(Decimal("1e100000000000000000000000000")) { t.Fatal("large exponent equality") }
    if !Decimal("-0.0").Equal(Decimal("0")) || !Decimal("1.00").Equal(Decimal("1")) { t.Fatal("decimal numeric equality") }
    client, err := NewTypesClient(Config{Endpoint:"https://example.test"})
    if err != nil { t.Fatal(err) }
    for _, text := range []string{`{"mode":2147483648}`, `{"names":["a","a"]}`, `{"integers":[1,1]}`, `{"decimal":"1.5"}`, `{"integer":1.5}`, `{"integers":[123456789012345678901234567890,123456789012345678901234567890]}`, `{"decimals":[1,1.0]}`} {
        decoder := json.NewDecoder(strings.NewReader(text)); decoder.UseNumber()
        var value any
        if err := decoder.Decode(&value); err != nil { t.Fatal(err) }
        var output Output
        if err := decodeModel(client.schema, "example.types#Output", value, &output); err == nil { t.Fatal("accepted", text) }
    }
    output := &Output{Integer:big.NewInt(1), Decimal:Decimal("NaN")}
    if _, err := encodeValue(client.schema, "example.types#Output", reflect.ValueOf(output), ""); err == nil { t.Fatal("nonfinite decimal accepted") }
    output.Decimal = Decimal("1.00000000000000000000000001")
    output.SetNames(Names{"duplicate", "duplicate"})
    if _, err := encodeValue(client.schema, "example.types#Output", reflect.ValueOf(output), ""); err == nil { t.Fatal("duplicate set accepted") }
    for _, invalid := range []string{"", "NaN", "+1", "01", "1/2", "null", "[1]", "1 ", "1\n"} {
        if _, err := ParseDecimal(invalid); err == nil { t.Fatal("invalid decimal accepted", invalid) }
    }
    for i := 1; i < 120; i++ {
        text := strings.Repeat("9", i) + "." + strings.Repeat("0", i) + "1"
        value, err := ParseDecimal(text)
        if err != nil { t.Fatal(err) }
        data, err := json.Marshal(value)
        if err != nil || string(data) != text { t.Fatal(text, string(data), err) }
    }
}

