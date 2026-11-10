"""Timestamps that keep the nanosecond the Crucible wire format carries."""
from __future__ import annotations

import re
from datetime import datetime, timedelta, timezone, tzinfo as _tzinfo
from decimal import Decimal
from typing import Any

_UTC = timezone.utc
_EPOCH = datetime(1970, 1, 1, tzinfo=_UTC)
_BILLION = 1_000_000_000
_ASCII = re.ASCII
_DATE_TIME = re.compile(
    r"(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:([Zz])|([+-])(\d{2}):(\d{2}))?", _ASCII)
# JSON number syntax: what an epoch-seconds query parameter or header holds.
_JSON_NUMBER = re.compile(r"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?", _ASCII)


class Timestamp(datetime):
    """A timezone-aware ``datetime`` that also carries the nanosecond.

    ``datetime`` stops at the microsecond; ``Timestamp`` adds ``nanosecond``
    (0 to 999, the part below the microsecond, as pandas does), so a wire
    timestamp with nine fractional digits survives decoding and encoding. It
    is a ``datetime``: code that reads one as a ``datetime`` keeps working,
    and a plain aware ``datetime`` is accepted wherever a timestamp is sent.

    Equality, ordering, hashing, ``isoformat``, ``repr``, ``replace``,
    ``astimezone`` and ``+``/``-`` with a ``timedelta`` include the nanosecond.
    Other inherited operations see the microsecond only. ``to_datetime()``
    drops the nanosecond and is the one lossy conversion.

    Years 1 through 9999 only, as ``datetime`` has.
    """
    __slots__ = ("_nanosecond",)
    _nanosecond: int

    def __new__(cls, year: int, month: int, day: int, hour: int = 0, minute: int = 0, second: int = 0,
                microsecond: int = 0, tzinfo: _tzinfo | None = _UTC, *, nanosecond: int = 0,
                fold: int = 0) -> Timestamp:
        if isinstance(nanosecond, bool) or not isinstance(nanosecond, int) or not 0 <= nanosecond <= 999:
            raise ValueError("nanosecond must be an integer from 0 to 999")
        self = super().__new__(cls, year, month, day, hour, minute, second, microsecond, tzinfo, fold=fold)
        self._nanosecond = nanosecond
        return self

    @classmethod
    def from_datetime(cls, value: datetime) -> Timestamp:
        """The same instant as ``value`` and, for a ``Timestamp``, the same nanosecond."""
        if not isinstance(value, datetime):
            raise TypeError("from_datetime requires a datetime")
        return cls(value.year, value.month, value.day, value.hour, value.minute, value.second,
                   value.microsecond, value.tzinfo, nanosecond=getattr(value, "_nanosecond", 0), fold=value.fold)

    @classmethod
    def from_seconds(cls, seconds: int, nanoseconds: int = 0) -> Timestamp:
        """UTC ``seconds`` since the Unix epoch plus ``nanoseconds`` (0 to 999999999), both exact."""
        if isinstance(seconds, bool) or not isinstance(seconds, int) or isinstance(nanoseconds, bool) \
                or not isinstance(nanoseconds, int) or not 0 <= nanoseconds < _BILLION:
            raise ValueError("Timestamp requires integer seconds and 0 to 999999999 nanoseconds")
        try:
            base = _EPOCH + timedelta(seconds=seconds, microseconds=nanoseconds // 1000)
        except OverflowError:
            raise ValueError("timestamp outside years 1 through 9999") from None
        return cls(base.year, base.month, base.day, base.hour, base.minute, base.second, base.microsecond,
                   _UTC, nanosecond=nanoseconds % 1000)

    @classmethod
    def from_epoch(cls, value: int | float | Decimal | str) -> Timestamp:
        """Epoch seconds as an exact number. A float is read as its shortest decimal text.

        A value finer than a nanosecond, or non-finite, raises ``ValueError``.
        """
        if isinstance(value, bool):
            raise ValueError("epoch seconds require a number")
        if isinstance(value, str):
            if not _JSON_NUMBER.fullmatch(value):
                raise ValueError("invalid epoch seconds")
            value = Decimal(value)
        elif isinstance(value, float):
            value = Decimal(repr(value))
        elif isinstance(value, int):
            value = Decimal(value)
        elif not isinstance(value, Decimal):
            raise ValueError("epoch seconds require a number")
        if not value.is_finite():
            raise ValueError("epoch seconds must be finite")
        sign, digits, exponent = value.as_tuple()
        assert isinstance(exponent, int)
        count = int("".join(map(str, digits)))
        if count == 0:
            return cls.from_seconds(0)
        shift = exponent + 9
        if shift < 0:
            # Tiny or nonzero digits below a nanosecond cannot be kept; reject without building 10**shift.
            if -shift > len(digits) or count % 10 ** -shift:
                raise ValueError("timestamp finer than a nanosecond")
            count //= 10 ** -shift
        elif shift > 40:
            raise ValueError("timestamp outside years 1 through 9999")
        else:
            count *= 10 ** shift
        seconds, nanoseconds = divmod(-count if sign else count, _BILLION)
        return cls.from_seconds(seconds, nanoseconds)

    @classmethod
    def parse(cls, text: str) -> Timestamp:
        """An RFC 3339 ``date-time``, with up to nine fractional digits; no offset means UTC.

        Digits past the ninth must be zero. The offset is kept as the zone.
        """
        match = _DATE_TIME.fullmatch(text) if isinstance(text, str) else None
        if match is None:
            raise ValueError("invalid date-time")
        year, month, day, hour, minute, second = (int(part) for part in match.groups()[:6])
        fraction = match.group(7) or ""
        if len(fraction) > 9 and fraction[9:].strip("0"):
            raise ValueError("timestamp finer than a nanosecond")
        digits = (fraction + "000000000")[:9]
        zone = _UTC
        if match.group(9):
            offset_hours, offset_minutes = int(match.group(10)), int(match.group(11))
            if offset_hours > 23 or offset_minutes > 59:
                raise ValueError("invalid date-time offset")
            offset = timedelta(hours=offset_hours, minutes=offset_minutes)
            zone = timezone(-offset if match.group(9) == "-" else offset)
        return cls(year, month, day, hour, minute, second, int(digits[:6]), zone, nanosecond=int(digits[6:]))

    @property
    def nanosecond(self) -> int:
        """The part of the second below the microsecond, 0 to 999."""
        return self._nanosecond

    @property
    def seconds(self) -> int:
        """Whole seconds since the Unix epoch, rounded toward the past."""
        return self._parts()[0]

    @property
    def nanoseconds(self) -> int:
        """The fraction of a second in nanoseconds, 0 to 999999999."""
        return self.microsecond * 1000 + self._nanosecond

    def to_datetime(self) -> datetime:
        """A plain ``datetime`` with the nanosecond dropped: the lossy conversion."""
        return datetime(self.year, self.month, self.day, self.hour, self.minute, self.second,
                        self.microsecond, self.tzinfo, fold=self.fold)

    def to_epoch(self) -> Decimal:
        """Epoch seconds as an exact ``Decimal``."""
        seconds, nanoseconds = self._parts()
        return Decimal(_epoch_text(seconds, nanoseconds))

    def _parts(self) -> tuple[int, int]:
        if self.tzinfo is None or self.utcoffset() is None:
            raise TypeError("timestamps must be timezone-aware datetime values")
        delta = datetime.__sub__(self, _EPOCH)
        return delta.days * 86400 + delta.seconds, delta.microseconds * 1000 + self._nanosecond

    def _carry(self, plain: datetime) -> Timestamp:
        return Timestamp(plain.year, plain.month, plain.day, plain.hour, plain.minute, plain.second,
                         plain.microsecond, plain.tzinfo, nanosecond=self._nanosecond, fold=plain.fold)

    def replace(self, *args: Any, nanosecond: int | None = None, **kwargs: Any) -> Timestamp:  # type: ignore[override]
        plain = datetime.replace(self.to_datetime(), *args, **kwargs)
        result = self._carry(plain)
        if nanosecond is not None:
            result = Timestamp(plain.year, plain.month, plain.day, plain.hour, plain.minute, plain.second,
                               plain.microsecond, plain.tzinfo, nanosecond=nanosecond, fold=plain.fold)
        return result

    __replace__ = replace

    def astimezone(self, tz: _tzinfo | None = None) -> Timestamp:
        return self._carry(datetime.astimezone(self.to_datetime(), tz))

    def __add__(self, other: Any) -> Any:
        result = datetime.__add__(self.to_datetime(), other)
        return result if result is NotImplemented else self._carry(result)

    __radd__ = __add__

    def __sub__(self, other: Any) -> Any:
        result = datetime.__sub__(self.to_datetime(), other.to_datetime() if isinstance(other, Timestamp) else other)
        return self._carry(result) if isinstance(result, datetime) else result

    def _order(self, other: Any) -> int | None:
        if not isinstance(other, datetime):
            return None
        mine, theirs = self.to_datetime(), other.to_datetime() if isinstance(other, Timestamp) else other
        if datetime.__eq__(mine, theirs):
            difference = self._nanosecond - getattr(other, "_nanosecond", 0)
            return (difference > 0) - (difference < 0)
        return -1 if datetime.__lt__(mine, theirs) else 1

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, datetime):
            return NotImplemented
        mine, theirs = self.to_datetime(), other.to_datetime() if isinstance(other, Timestamp) else other
        return bool(datetime.__eq__(mine, theirs)) and self._nanosecond == getattr(other, "_nanosecond", 0)

    def __ne__(self, other: object) -> bool:
        result = self.__eq__(other)
        return result if result is NotImplemented else not result

    def __lt__(self, other: Any) -> bool:
        order = self._order(other)
        return NotImplemented if order is None else order < 0

    def __le__(self, other: Any) -> bool:
        order = self._order(other)
        return NotImplemented if order is None else order <= 0

    def __gt__(self, other: Any) -> bool:
        order = self._order(other)
        return NotImplemented if order is None else order > 0

    def __ge__(self, other: Any) -> bool:
        order = self._order(other)
        return NotImplemented if order is None else order >= 0

    def __hash__(self) -> int:
        base = datetime.__hash__(self.to_datetime())
        return hash((base, self._nanosecond)) if self._nanosecond else base

    def isoformat(self, sep: str = "T", timespec: str = "auto") -> str:
        """As ``datetime.isoformat``; a nonzero nanosecond adds three digits, and ``"nanoseconds"`` always does."""
        if timespec == "nanoseconds" or (timespec == "auto" and self._nanosecond):
            text = datetime.isoformat(self.to_datetime(), sep, "microseconds")
            return re.sub(r"(\.\d{6})", lambda found: f"{found.group(1)}{self._nanosecond:03d}", text, count=1)
        return datetime.isoformat(self.to_datetime(), sep, "auto" if timespec == "nanoseconds" else timespec)

    def __repr__(self) -> str:
        text = datetime.__repr__(self)
        return f"{text[:-1]}, nanosecond={self._nanosecond})" if self._nanosecond else text

    def __reduce_ex__(self, protocol: Any) -> Any:
        return (_unpickle, (self.year, self.month, self.day, self.hour, self.minute, self.second,
                            self.microsecond, self.tzinfo, self._nanosecond, self.fold))

    def __reduce__(self) -> Any:
        return self.__reduce_ex__(4)


def _unpickle(year: int, month: int, day: int, hour: int, minute: int, second: int, microsecond: int,
              tzinfo: _tzinfo | None, nanosecond: int, fold: int) -> Timestamp:
    return Timestamp(year, month, day, hour, minute, second, microsecond, tzinfo, nanosecond=nanosecond, fold=fold)


def _epoch_text(seconds: int, nanoseconds: int) -> str:
    """Exact decimal epoch seconds: the fraction trimmed, and ``.0`` for whole seconds, as a float printed."""
    total = seconds * _BILLION + nanoseconds
    whole, fraction = divmod(abs(total), _BILLION)
    return f"{'-' if total < 0 else ''}{whole}.{f'{fraction:09d}'.rstrip('0') or '0'}"


def _date_time_text(seconds: int, nanoseconds: int) -> str:
    """RFC 3339 in UTC, the fraction trimmed of trailing zeros, as the wire crate writes it."""
    try:
        utc = _EPOCH + timedelta(seconds=seconds)
    except OverflowError:
        raise ValueError("timestamp outside years 1 through 9999") from None
    fraction = f".{nanoseconds:09d}".rstrip("0") if nanoseconds else ""
    return (f"{utc.year:04d}-{utc.month:02d}-{utc.day:02d}T{utc.hour:02d}:{utc.minute:02d}:{utc.second:02d}"
            f"{fraction}Z")
