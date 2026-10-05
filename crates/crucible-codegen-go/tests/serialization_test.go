package weather

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

// JSON numbers must remain exact even when the host fixture parser uses floats.
func exactJSON(text string) (any, error) {
	decoder := json.NewDecoder(strings.NewReader(text))
	decoder.UseNumber()
	var value any
	err := decoder.Decode(&value)
	return value, err
}

func jsonEqual(a, b any) bool {
	if n, ok := a.(json.Number); ok {
		m, ok := b.(json.Number)
		if !ok {
			return false
		}
		x, ok := new(big.Rat).SetString(string(n))
		if !ok {
			return false
		}
		y, ok := new(big.Rat).SetString(string(m))
		return ok && x.Cmp(y) == 0
	}
	if x, ok := a.(map[string]any); ok {
		y, ok := b.(map[string]any)
		if !ok || len(x) != len(y) {
			return false
		}
		for key, v := range x {
			w, present := y[key]
			if !present || !jsonEqual(v, w) {
				return false
			}
		}
		return true
	}
	if x, ok := a.([]any); ok {
		y, ok := b.([]any)
		if !ok || len(x) != len(y) {
			return false
		}
		for i, v := range x {
			if !jsonEqual(v, y[i]) {
				return false
			}
		}
		return true
	}
	return reflect.DeepEqual(a, b)
}

func TestSharedSerialization(t *testing.T) {
	data, err := os.ReadFile("serialization.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		ID, Target, Wire string
		Reject           bool
		Expect           struct {
			JSON    string
			States  map[string]string
			BlobHex string
			Instant *struct {
				Member, Seconds string
				Nanoseconds     int
			}
		}
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		t.Run(test.ID, func(t *testing.T) {
			var native any
			switch test.Target {
			case "example.weather#CitySummary":
				native = new(CitySummary)
			case "example.weather#Photo":
				native = new(Photo)
			case "example.weather#Sky":
				native = new(Sky)
			case "example.weather#Location":
				native = new(Location)
			case "example.weather#Forecast":
				native = new(Forecast)
			default:
				t.Fatalf("unhandled shared target %s", test.Target)
			}
			wire, err := exactJSON(test.Wire)
			if err != nil {
				t.Fatal(err)
			}
			err = decodeModel(wireSchema, test.Target, wire, native)
			if test.Reject {
				if err == nil {
					t.Fatal("invalid wire accepted")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			encoded, err := encodeValue(wireSchema, test.Target, reflect.ValueOf(native), "")
			if err != nil {
				t.Fatal(err)
			}
			bytes, err := json.Marshal(encoded)
			if err != nil {
				t.Fatal(err)
			}
			actual, err := exactJSON(string(bytes))
			if err != nil {
				t.Fatal(err)
			}
			expected, err := exactJSON(test.Expect.JSON)
			if err != nil {
				t.Fatal(err)
			}
			if !jsonEqual(actual, expected) {
				t.Fatalf("got %s want %s", bytes, test.Expect.JSON)
			}
			for member, state := range test.Expect.States {
				field := reflect.ValueOf(native).Elem().FieldByName(wireSchema.Shapes[test.Target].Members[member].Attribute)
				got := "value"
				if _, optional := field.Interface().(optionalField); optional {
					got = "absent"
					if field.FieldByName("Set").Bool() {
						got = "value"
						if field.FieldByName("Null").Bool() {
							got = "null"
						}
					}
				}
				if got != state {
					t.Fatalf("%s state %s want %s", member, got, state)
				}
			}
			if test.Expect.BlobHex != "" && hex.EncodeToString(*native.(*Photo)) != test.Expect.BlobHex {
				t.Fatal("blob bytes differ")
			}
			if instant := test.Expect.Instant; instant != nil {
				field := reflect.ValueOf(native).Elem().FieldByName(wireSchema.Shapes[test.Target].Members[instant.Member].Attribute)
				value := field.FieldByName("Value").Interface().(time.Time)
				if fmt.Sprint(value.Unix()) != instant.Seconds || value.Nanosecond() != instant.Nanoseconds {
					t.Fatalf("instant %s", value)
				}
			}
		})
	}
}
