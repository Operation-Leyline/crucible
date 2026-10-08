from decimal import Decimal
import random
import unittest

import httpx
from types_client import TypesClient, Mode, Output
from types_client import models
from types_client.runtime import _Codec, _json


class ExtendedTypeTests(unittest.TestCase):
    def test_native_decimals_integer_enums_sparse_and_defaults(self):
        body = b'{"mode":42,"names":["a","b"],"sparseNames":["a",null],"sparseMap":{"known":-1,"unknown":42,"null":null}}'
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(200, content=body))) as http:
            output = TypesClient(endpoint="https://example.test", http=http).read()
        self.assertEqual(output.integer, 12345678901234567890123456789012345678901234567890)
        self.assertEqual(output.decimal, Decimal("12345678901234567890.12345678901234567890123456789"))
        self.assertEqual(output.mode, Mode(42))
        self.assertEqual(output.sparse_names, ["a", None])
        self.assertEqual(output.sparse_map, {"known": Mode.OFF, "unknown": Mode(42), "null": None})
        codec = _Codec(models._SCHEMA, models._CLASSES)
        wire = _json(codec.convert("example.types#Output", output))
        self.assertIn('"decimal":12345678901234567890.12345678901234567890123456789', wire)
        self.assertIn('"mode":42', wire)

    def test_exact_number_roundtrips_over_many_magnitudes(self):
        import json
        codec = _Codec(models._SCHEMA, models._CLASSES)
        rng = random.Random(91827)
        for _ in range(120):
            integer = rng.randrange(-(10 ** 150), 10 ** 150)
            decimal = Decimal(f"{integer}e{rng.randrange(-200, 201)}")
            original = Output(integer=integer, decimal=decimal)
            body = _json(codec.convert("example.types#Output", original))
            decoded = codec.convert("example.types#Output", json.loads(body, parse_float=Decimal), decode=True)
            self.assertEqual(decoded.integer, integer)
            self.assertEqual(decoded.decimal, decimal)

    def test_response_headers_decode_int_enum_big_integer_and_big_decimal(self):
        codec = _Codec(models._SCHEMA, models._CLASSES)
        big = 12345678901234567890123456789012345678901234567890
        self.assertEqual(codec.read_header({"target": "example.types#Mode"}, "2"), Mode(2))
        self.assertEqual(codec.read_header({"target": "smithy.api#BigInteger"}, str(big)), big)
        self.assertEqual(codec.read_header({"target": "smithy.api#BigDecimal"}, "1.5"), Decimal("1.5"))

    def test_offset_less_date_time_decodes_aware_and_round_trips(self):
        from datetime import timezone
        from types_client.runtime import _timestamp
        decoded = _timestamp("2026-10-01T12:30:00", "date-time", True)
        self.assertEqual(decoded.tzinfo, timezone.utc)
        self.assertEqual(_timestamp(decoded, "date-time", False), "2026-10-01T12:30:00Z")

    def test_invalid_precision_and_collection_values_fail(self):
        codec = _Codec(models._SCHEMA, models._CLASSES)
        for value in (True, 0.5, 2 ** 31):
            with self.assertRaises(ValueError):
                codec.convert("example.types#Mode", value)
        for value in (0.1, Decimal("NaN"), Decimal("Infinity")):
            with self.assertRaises(ValueError):
                codec.convert("smithy.api#BigDecimal", value)
        for decode in (False, True):
            for shape, values in (("Integers", [10 ** 80, 10 ** 80]), ("Decimals", [Decimal("1"), Decimal("1.00")])):
                with self.assertRaisesRegex(ValueError, "duplicate"):
                    codec.convert("example.types#" + shape, values, decode=decode)
            codec.convert("example.types#Integers", [10 ** 80, 10 ** 80 + 1], decode=decode)
            codec.convert("example.types#Decimals", [Decimal("1"), Decimal("1.0000000000000000000000001")], decode=decode)
            with self.assertRaisesRegex(ValueError, "duplicate"):
                codec.convert("example.types#Names", ["same", "same"], decode=decode)
            with self.assertRaisesRegex(ValueError, "non-sparse"):
                codec.convert("example.types#Names", [None], decode=decode)


if __name__ == "__main__":
    unittest.main()
