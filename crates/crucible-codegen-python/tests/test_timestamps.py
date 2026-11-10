"""Timestamps are lossless to the nanosecond, and native datetimes encode as before."""
import copy
import json
import pickle
import unittest
from datetime import datetime, timedelta, timezone
from decimal import Decimal

import httpx
from weather_client import WeatherClient, Timestamp
from weather_client import models as m
from weather_client.runtime import _Codec, _json, _timestamp

UTC = timezone.utc
CITY = "example.weather#CitySummary"
SCHEMA = {"shapes": {"example#Time": {"kind": "timestamp", "format": "date-time"},
                     "example#Times": {"kind": "list", "member": {"target": "example#Time", "format": "date-time"}}}}


def city_codec():
    return _Codec(m._SCHEMA, m._CLASSES)


def read_city(body: str):
    transport = httpx.MockTransport(lambda request: httpx.Response(200, content=body.encode()))
    with httpx.Client(transport=transport) as http:
        with WeatherClient(http=http, token="token") as client:
            return client.get_city(m.GetCityInput(city_id="c-0a1b2c3d")).city


def city_body(**members):
    extra = "".join(f',"{k}":{v}' for k, v in members.items())
    return '{"city":{"id":"c-0a1b2c3d","name":"Oslo"' + extra + "}}"


class DateTimeRoundTrip(unittest.TestCase):
    def test_microsecond_and_nanosecond_body_members_survive(self):
        for text in ("2026-10-01T12:30:00Z", "2026-10-01T12:30:00.123456Z", "2026-10-01T12:30:00.123456789Z",
                     "2026-10-01T12:30:00.000000001Z", "2026-10-01T12:30:00.999999999Z"):
            with self.subTest(text=text):
                city = read_city(city_body(updatedAt=json.dumps(text)))
                self.assertIsInstance(city.updated_at, Timestamp)
                self.assertEqual(_json(city_codec().convert(CITY, city, strict=False))
                                 .count(text.removesuffix("Z")), 1)

    def test_nine_digit_fraction_is_parsed_without_datetime_fromisoformat(self):
        # datetime.fromisoformat drops digits past six or rejects nine; the runtime parses the fraction itself,
        # so the result does not depend on the interpreter version.
        value = _timestamp("2026-10-01T12:30:00.123456789Z", "date-time", True)
        self.assertEqual((value.microsecond, value.nanosecond, value.nanoseconds), (123456, 789, 123456789))

    def test_query_parameter_carries_the_nanosecond(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, content=city_body().encode())
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            with WeatherClient(http=http, token="token") as client:
                client.get_city(m.GetCityInput(city_id="c-0a1b2c3d", at=Timestamp(2026, 10, 1, 12, 30, 0, 123456, nanosecond=789)))
                client.get_city(m.GetCityInput(city_id="c-0a1b2c3d", at=Timestamp(2026, 10, 1, 12, 30, 0, 123456)))
        self.assertEqual(sent[0].url.params["at"], "2026-10-01T12:30:00.123456789Z")
        self.assertEqual(sent[1].url.params["at"], "2026-10-01T12:30:00.123456Z")

    def test_header_values_carry_the_nanosecond(self):
        codec = _Codec(SCHEMA, {})
        member = {"target": "example#Time"}
        value = codec.read_header(member, "2026-10-01T12:30:00.123456789+00:00", fmt="date-time")
        self.assertEqual(value.nanosecond, 789)
        self.assertEqual(codec.bound(member, value, header=True, fmt="date-time"), "2026-10-01T12:30:00.123456789Z")
        listed = codec.read_header({"target": "example#Times"}, "2026-10-01T12:30:00.000000001Z, 2026-10-01T12:30:00.5Z")
        self.assertEqual([item.nanoseconds for item in listed], [1, 500000000])

    def test_offsets_are_normalised_to_utc_when_sent(self):
        value = _timestamp("1970-01-01T01:00:00.000000001+01:00", "date-time", True)
        self.assertEqual(_timestamp(value, "date-time", False), "1970-01-01T00:00:00.000000001Z")

    def test_finer_than_a_nanosecond_is_refused_but_trailing_zeros_are_not(self):
        self.assertEqual(_timestamp("2026-10-01T12:30:00.1234567890Z", "date-time", True).nanoseconds, 123456789)
        for text in ("2026-10-01T12:30:00.1234567891Z", "2026-02-30T00:00:00Z", "2026-10-01T12:30:00.Z", "nope"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                _timestamp(text, "date-time", True)


class EpochRoundTrip(unittest.TestCase):
    def test_epoch_members_are_exact_decimals(self):
        for text, seconds, nanoseconds in (("1700000000", 1700000000, 0), ("1.25", 1, 250000000),
                                           ("1700000000.123456", 1700000000, 123456000),
                                           ("1700000000.123456789", 1700000000, 123456789),
                                           ("-0.000000125", -1, 999999875), ("0.000000001", 0, 1)):
            with self.subTest(text=text):
                city = read_city(city_body(createdAt=text))
                self.assertEqual((city.created_at.seconds, city.created_at.nanoseconds), (seconds, nanoseconds))
                wire = _json(city_codec().convert(CITY, city))
                self.assertEqual(json.loads(wire, parse_float=Decimal)["createdAt"], Decimal(text))

    def test_epoch_query_and_header_text_is_exact(self):
        codec = _Codec({"shapes": {"example#Time": {"kind": "timestamp", "format": "epoch-seconds"}}}, {})
        member = {"target": "example#Time"}
        value = codec.read_header(member, "1700000000.123456789", fmt="epoch-seconds")
        self.assertEqual(value.nanoseconds, 123456789)
        self.assertEqual(codec.bound(member, value, fmt="epoch-seconds"), "1700000000.123456789")

    def test_floats_and_decimals_are_accepted_but_finer_than_a_nanosecond_is_not(self):
        self.assertEqual(Timestamp.from_epoch(1.5).nanoseconds, 500000000)
        self.assertEqual(Timestamp.from_epoch(Decimal("1.5E+3")).seconds, 1500)
        for bad in ("1.0000000001", "1e-10", "NaN", "Infinity", " 1", "1_0", "01", "", "1e400", True, None):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                Timestamp.from_epoch(bad)  # type: ignore[arg-type]
        for bad_number in (float("nan"), float("inf"), Decimal("NaN")):
            with self.subTest(bad=bad_number), self.assertRaises(ValueError):
                Timestamp.from_epoch(bad_number)


class NativeInput(unittest.TestCase):
    """A plain aware datetime encodes exactly as it did before Timestamp existed."""

    def test_date_time_output_is_the_wire_form(self):
        for value, expected in ((datetime(2026, 10, 1, 12, 30, tzinfo=UTC), "2026-10-01T12:30:00Z"),
                                (datetime(2026, 10, 1, 12, 30, 0, 120000, tzinfo=UTC), "2026-10-01T12:30:00.12Z"),
                                (datetime(2026, 10, 1, 12, 30, 0, 1, tzinfo=UTC), "2026-10-01T12:30:00.000001Z"),
                                (datetime(2026, 10, 1, 14, 30, tzinfo=timezone(timedelta(hours=2))), "2026-10-01T12:30:00Z"),
                                (datetime(7, 1, 1, tzinfo=UTC), "0007-01-01T00:00:00Z")):
            with self.subTest(expected=expected):
                self.assertEqual(_timestamp(value, "date-time", False), expected)

    def test_epoch_output_is_unchanged_in_text(self):
        for value in (datetime(2023, 11, 14, 22, 13, 20, tzinfo=UTC), datetime(2023, 11, 14, 22, 13, 20, 250000, tzinfo=UTC),
                      datetime(1969, 12, 31, 23, 59, 59, 500000, tzinfo=UTC)):
            with self.subTest(value=value):
                encoded = _timestamp(value, "epoch-seconds", False)
                self.assertEqual(_json(encoded), json.dumps(value.timestamp()))
                self.assertEqual(str(encoded), str(value.timestamp()))

    def test_http_date_and_naive_inputs(self):
        value = datetime(2019, 12, 16, 23, 48, 18, tzinfo=UTC)
        self.assertEqual(_timestamp(value, "http-date", False), "Mon, 16 Dec 2019 23:48:18 GMT")
        self.assertEqual(_timestamp("Mon, 16 Dec 2019 23:48:18 GMT", "http-date", True), value)
        for fmt in ("date-time", "epoch-seconds", "http-date"):
            with self.subTest(fmt=fmt), self.assertRaises(TypeError):
                _timestamp(datetime(2026, 10, 1), fmt, False)

    def test_native_datetime_in_a_request(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, content=city_body().encode())
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            with WeatherClient(http=http, token="token") as client:
                client.get_city(m.GetCityInput(city_id="c-0a1b2c3d", at=datetime(2026, 10, 1, 12, 30, 0, 120000, tzinfo=UTC)))
        self.assertEqual(sent[0].url.params["at"], "2026-10-01T12:30:00.12Z")

    def test_decoded_values_read_as_datetimes(self):
        city = read_city(city_body(updatedAt='"2026-10-01T12:30:00.123456789Z"'))
        self.assertIsInstance(city.updated_at, datetime)
        self.assertEqual(city.updated_at.year, 2026)
        self.assertEqual(city.updated_at.utcoffset(), timedelta(0))
        self.assertEqual(city.updated_at.to_datetime(), datetime(2026, 10, 1, 12, 30, 0, 123456, tzinfo=UTC))


class TimestampType(unittest.TestCase):
    def test_construction(self):
        value = Timestamp(2026, 10, 1, 12, 30, 0, 123456, nanosecond=789)
        self.assertEqual((value.tzinfo, value.nanosecond, value.nanoseconds), (UTC, 789, 123456789))
        self.assertEqual(Timestamp.from_seconds(1790857800, 123456789), value)
        self.assertEqual(Timestamp.parse("2026-10-01T12:30:00.123456789Z"), value)
        self.assertEqual(value.seconds, 1790857800)
        for bad in (-1, 1000, True, 1.0):
            with self.subTest(nanosecond=bad), self.assertRaises(ValueError):
                Timestamp(2026, 10, 1, nanosecond=bad)  # type: ignore[arg-type]
        for seconds, nanoseconds in ((0, -1), (0, 10 ** 9), (2 ** 60, 0), (-2 ** 60, 0), (True, 0), (1.5, 0)):
            with self.subTest(seconds=seconds, nanoseconds=nanoseconds), self.assertRaises(ValueError):
                Timestamp.from_seconds(seconds, nanoseconds)  # type: ignore[arg-type]

    def test_conversions(self):
        plain = datetime(2026, 10, 1, 12, 30, 0, 123456, tzinfo=UTC)
        value = Timestamp(2026, 10, 1, 12, 30, 0, 123456, nanosecond=789)
        self.assertEqual(Timestamp.from_datetime(plain), plain)
        self.assertEqual(Timestamp.from_datetime(plain).nanosecond, 0)
        self.assertEqual(Timestamp.from_datetime(value).nanosecond, 789)
        lossy = value.to_datetime()
        self.assertIs(type(lossy), datetime)
        self.assertEqual(lossy, plain)
        self.assertEqual(value.to_epoch(), Decimal("1790857800.123456789"))
        self.assertEqual(Timestamp.from_epoch(value.to_epoch()), value)
        with self.assertRaises(TypeError):
            Timestamp.from_datetime("2026-10-01")  # type: ignore[arg-type]

    def test_a_nanosecond_makes_values_differ(self):
        a = Timestamp(2026, 10, 1, 12, 30, 0, 5, nanosecond=1)
        b = Timestamp(2026, 10, 1, 12, 30, 0, 5, nanosecond=2)
        plain = datetime(2026, 10, 1, 12, 30, 0, 5, tzinfo=UTC)
        self.assertNotEqual(a, b)
        self.assertTrue(a < b and b > a and a <= b and b >= a and not b < a)
        self.assertEqual(Timestamp.from_datetime(plain), plain)
        self.assertEqual(hash(Timestamp.from_datetime(plain)), hash(plain))
        self.assertNotEqual(a, plain)
        self.assertTrue(plain < a and a > plain and plain != a)
        self.assertEqual(len({a, b, copy.copy(a)}), 2)
        self.assertEqual(sorted([b, plain, a]), [plain, a, b])
        self.assertFalse(a == "2026")
        with self.assertRaises(TypeError):
            a < datetime(2026, 10, 1)

    def test_text_forms(self):
        value = Timestamp(2026, 10, 1, 12, 30, 0, 123456, nanosecond=789)
        self.assertEqual(value.isoformat(), "2026-10-01T12:30:00.123456789+00:00")
        self.assertEqual(str(value), "2026-10-01 12:30:00.123456789+00:00")
        self.assertEqual(repr(value), "Timestamp(2026, 10, 1, 12, 30, 0, 123456, tzinfo=datetime.timezone.utc, nanosecond=789)")
        whole = Timestamp(2026, 10, 1, 12, 30, 0, 123456)
        self.assertEqual(whole.isoformat(), "2026-10-01T12:30:00.123456+00:00")
        self.assertNotIn("nanosecond", repr(whole))
        self.assertEqual(whole.isoformat(timespec="nanoseconds"), "2026-10-01T12:30:00.123456000+00:00")

    def test_derived_values_keep_the_nanosecond(self):
        value = Timestamp(2026, 10, 1, 12, 30, 0, 123456, nanosecond=789)
        self.assertEqual((value + timedelta(seconds=1)).nanosecond, 789)
        self.assertEqual((timedelta(seconds=1) + value).second, 1)
        self.assertEqual((value - timedelta(days=1)).nanosecond, 789)
        self.assertIsInstance(value - Timestamp(2026, 10, 1), timedelta)
        self.assertEqual(value.replace(minute=1).nanosecond, 789)
        self.assertEqual(value.replace(nanosecond=3).nanosecond, 3)
        shifted = value.astimezone(timezone(timedelta(hours=2)))
        self.assertEqual((shifted.hour, shifted.nanosecond), (14, 789))
        self.assertEqual(_timestamp(shifted, "date-time", False), "2026-10-01T12:30:00.123456789Z")

    def test_copy_and_pickle_keep_the_nanosecond(self):
        value = Timestamp(2026, 10, 1, 12, 30, 0, 123456, timezone(timedelta(hours=5)), nanosecond=789)
        for clone in (copy.copy(value), copy.deepcopy(value), pickle.loads(pickle.dumps(value))):
            self.assertEqual((clone, clone.nanosecond, clone.utcoffset()), (value, 789, timedelta(hours=5)))

    def test_range_follows_datetime(self):
        with self.assertRaises(ValueError):
            Timestamp.from_epoch("253402300800")
        self.assertEqual(Timestamp.from_epoch("-62135596800").year, 1)


class NonFiniteFloats(unittest.TestCase):
    def test_non_finite_floats_are_not_a_wire_value(self):
        codec = _Codec({"shapes": {"example#Number": {"kind": "double"}}}, {})
        for value in (float("nan"), float("inf"), float("-inf"), Decimal("NaN"), Decimal("-Infinity")):
            with self.subTest(value=value), self.assertRaises(ValueError):
                codec.bound({"target": "example#Number"}, value)
        for text in ("NaN", "Infinity", "-Infinity", "nan", "inf"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                codec.read_header({"target": "example#Number"}, text)
            with self.subTest(literal=text), self.assertRaises(ValueError):
                codec.convert("example#Number", text, decode=True, strict=True)
        self.assertEqual(codec.read_header({"target": "example#Number"}, "1.5"), 1.5)
        self.assertEqual(codec.bound({"target": "example#Number"}, 1.5), "1.5")


if __name__ == "__main__":
    unittest.main()
