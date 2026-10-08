using Example.Docs;
using Example.Docs.Models;

// Generated ToString is readable and never prints a sensitive value.
static class Program
{
    const string Hidden = "*** Sensitive Data Redacted ***";

    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    static void Equal(string expected, object? actual) => Check(expected == actual?.ToString(), $"expected <{expected}> but was <{actual}>");

    static void Main()
    {
        var input = new GreetInput
        {
            Name = "Ada",
            Password = "hunter2",
            Token = "tok-1",
            Secrets = ["s3cret", "other"],
            Names = ["a", "b"],
            Profile = new Profile { Nickname = "ada", Pin = "1234" },
            Choice = new Choice { Code = "c0de" },
            Mode = Mode.Fast,
        };
        var text = input.ToString();
        foreach (var secret in new[] { "hunter2", "tok-1", "s3cret", "other", "1234", "c0de" })
            Check(!text.Contains(secret), $"leaked {secret} in {text}");
        Equal($"GreetInput(Name=Ada, Password={Hidden}, Token={Hidden}, Secrets={Hidden}, Names=[a, b], "
            + $"Profile=Profile(Nickname=ada, Pin={Hidden}), Choice=Choice(Code={Hidden}), Mode=fast)", text);

        Equal("GreetInput(Name=x)", new GreetInput { Name = "x" });
        Equal("Profile(Nickname=null)", new Profile { Nickname = null });
        Equal("Choice(Label=hi)", new Choice { Label = "hi" });
        var unknown = new UnknownUnion("future", System.Text.Json.Nodes.JsonValue.Create("secret"));
        Equal($"UnknownUnion(Name=future, Value={Hidden})", unknown);
        Equal($"Choice(UnknownVariant=UnknownUnion(Name=future, Value={Hidden}))", new Choice { UnknownVariant = unknown });

        var error = new Unavailable { MessageValue = "down", RetryToken = "retry-me" };
        Equal($"Unavailable(MessageValue=down, RetryToken={Hidden})", error);
    }
}
