using System.Net;
using System.Text.Json.Nodes;
using Example.Instants;
using Example.Instants.Models;

// Timestamps are lossless to the nanosecond: a body member, a query parameter and a header, in date-time and
// epoch-seconds, and a DateTimeOffset converts without surprise.
static class Program
{
    static int checks;
    static void Check(bool value, string message) { checks++; if (!value) throw new Exception(message); }
    static void Throws<T>(Action action, string message) where T : Exception
    {
        try { action(); } catch (T) { checks++; return; }
        throw new Exception(message);
    }

    sealed class Handler : HttpMessageHandler
    {
        public HttpRequestMessage? Request;
        public string SentBody = "";
        public string Body = "{}";
        public Dictionary<string, string> Headers = [];
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Request = request;
            SentBody = request.Content is null ? "" : await request.Content.ReadAsStringAsync(token);
            var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(Body) };
            foreach (var (name, value) in Headers) response.Headers.Add(name, value);
            return response;
        }
    }

    static async Task Main()
    {
        var handler = new Handler();
        using var http = new HttpClient(handler);
        using var client = new InstantsClient(new() { Endpoint = "https://example.test", HttpClient = http });

        foreach (var (text, seconds, nanos, epoch) in new[] {
            ("2026-03-04T05:06:07Z", 1772600767L, 0, "1772600767"),
            ("2026-03-04T05:06:07.123456Z", 1772600767L, 123456000, "1772600767.123456"),
            ("2026-03-04T05:06:07.123456789Z", 1772600767L, 123456789, "1772600767.123456789"),
            ("2026-03-04T05:06:07.000000001Z", 1772600767L, 1, "1772600767.000000001"),
            ("1969-12-31T23:59:59.999999999Z", -1L, 999999999, "-0.000000001") })
        {
            var expected = new Timestamp(seconds, nanos);
            // Sent: query, header and body in both formats.
            await client.EchoAsync(new() { At = expected, Epoch = expected, AtHeader = expected, EpochHeader = expected, BodyAt = expected, BodyEpoch = expected });
            var request = handler.Request!;
            var query = System.Web.HttpUtility.ParseQueryString(request.RequestUri!.Query);
            Check(query["at"] == text, "query date-time " + text);
            Check(query["epoch"] == epoch, "query epoch " + text);
            Check(request.Headers.GetValues("X-At").Single() == text, "header date-time " + text);
            Check(request.Headers.GetValues("X-Epoch").Single() == epoch, "header epoch " + text);
            var sent = JsonNode.Parse(handler.SentBody)!;
            Check(sent["bodyAt"]!.GetValue<string>() == text, "body date-time " + text);
            Check(sent["bodyEpoch"]!.ToJsonString() == epoch, "body epoch " + text);

            // Received: the same text decodes to the same instant.
            handler.Body = $$"""{"bodyAt":"{{text}}","bodyEpoch":{{epoch}}}""";
            handler.Headers = new() { ["X-At"] = text, ["X-Epoch"] = epoch };
            var output = await client.EchoAsync(new());
            Check(output.BodyAt == expected && output.BodyEpoch == expected, "decoded body " + text);
            Check(output.AtHeader == expected && output.EpochHeader == expected, "decoded headers " + text);
            Check(output.BodyAt!.Value.Seconds == seconds && output.BodyAt.Value.Nanoseconds == nanos, "parts " + text);
            Check(output.BodyAt.Value.ToString() == text && output.BodyEpoch!.Value.ToEpoch().ToString() == epoch, "text forms " + text);
            handler.Body = "{}"; handler.Headers = [];
        }

        // An offset and an offset-less text are the same instant as UTC; trailing zeros past the ninth digit are accepted.
        Check(Timestamp.Parse("1970-01-01T01:00:00.000000001+01:00") == new Timestamp(0, 1), "offset");
        Check(Timestamp.Parse("2026-03-04T05:06:07") == new Timestamp(1772600767), "no offset is UTC");
        Check(Timestamp.Parse("2026-03-04T05:06:07.1234567890Z") == new Timestamp(1772600767, 123456789), "trailing zeros");
        Check(Timestamp.Parse("0000-01-01T00:00:00Z").Seconds == -62167219200, "year zero");
        Check(Timestamp.Parse("9999-12-31T23:59:59.999999999Z").ToString() == "9999-12-31T23:59:59.999999999Z", "last instant");
        foreach (var bad in new[] { "2026-03-04T05:06:07.1234567891Z", "2026-02-30T00:00:00Z", "2026-03-04T05:06:60Z", "2026-03-04", "2026-03-04T05:06:07.Z", "x", "" })
            Throws<FormatException>(() => Timestamp.Parse(bad), "invalid date-time accepted: " + bad);
        foreach (var bad in new[] { "1.0000000001", "1e-10" })
            Throws<FormatException>(() => Timestamp.FromEpoch(new BigDecimal(bad)), "epoch finer than a nanosecond accepted: " + bad);
        Throws<OverflowException>(() => Timestamp.FromEpoch(new BigDecimal("1e400")), "epoch out of range accepted");
        Check(Timestamp.FromEpoch(new BigDecimal("1.5E+3")) == new Timestamp(1500), "exponent form");
        Throws<ArgumentOutOfRangeException>(() => new Timestamp(0, 1_000_000_000), "nanoseconds out of range");
        Throws<ArgumentOutOfRangeException>(() => new Timestamp(0, -1), "negative nanoseconds");

        // A DateTimeOffset converts in exactly; converting back drops digits finer than a tick and never rounds up.
        var native = new DateTimeOffset(2026, 3, 4, 5, 6, 7, TimeSpan.FromHours(2)).AddTicks(1234567);
        Timestamp exact = native;
        Check(exact.Seconds == native.ToUnixTimeSeconds() && exact.Nanoseconds == 123456700, "implicit from DateTimeOffset");
        Check((DateTimeOffset)exact == native.ToUniversalTime(), "explicit to DateTimeOffset");
        Check((DateTimeOffset)new Timestamp(1772600767, 123456789) == DateTimeOffset.FromUnixTimeSeconds(1772600767).AddTicks(1234567), "lossy conversion truncates");
        Check((DateTimeOffset)new Timestamp(-1, 999999999) == DateTimeOffset.UnixEpoch.AddTicks(-1), "lossy conversion truncates toward the past");
        Check(Timestamp.FromDateTimeOffset(DateTimeOffset.UnixEpoch.AddTicks(-1)) == new Timestamp(-1, 999999900), "before the epoch");
        Throws<OverflowException>(() => _ = (DateTimeOffset)new Timestamp(-62167219200), "year zero has no DateTimeOffset");

        // A native DateTimeOffset encodes as it did before.
        await client.EchoAsync(new() { At = new DateTimeOffset(2026, 3, 4, 5, 6, 7, 120, TimeSpan.Zero), Epoch = DateTimeOffset.UnixEpoch.AddSeconds(1700000000) });
        var legacy = System.Web.HttpUtility.ParseQueryString(handler.Request!.RequestUri!.Query);
        Check(legacy["at"] == "2026-03-04T05:06:07.12Z" && legacy["epoch"] == "1700000000", "native input encodes as before");

        // Ordering and equality.
        Check(new Timestamp(1, 5) < new Timestamp(1, 6) && new Timestamp(2) > new Timestamp(1, 999999999) && new Timestamp(1, 5) != new Timestamp(1, 6), "ordering");
        Check(new Timestamp(1, 5).Equals(new Timestamp(1, 5)) && new Timestamp(1, 5).GetHashCode() == new Timestamp(1, 5).GetHashCode(), "equality");
        Check(new Timestamp(-62167219201).ToString() == "-62167219201", "out-of-text range prints seconds");
        Console.WriteLine($"C# timestamps passed ({checks} checks)");
    }
}
