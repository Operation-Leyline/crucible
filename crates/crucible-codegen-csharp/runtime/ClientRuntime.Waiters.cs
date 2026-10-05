using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;

internal sealed partial class ClientRuntime
{
    private JsonNode? QueryModel(string? target, JsonNode? value, string? format = null)
    {
        if (target is null || value is null) return null;
        var shape = Shape(target);
        switch (Text(shape["kind"]))
        {
            case "timestamp": return WriteTime(ReadTime(value,format ?? Text(shape["format"])),"epoch-seconds");
            case "structure": case "union": {
                var result = new JsonObject();
                foreach (var (name,member) in shape["members"]!.AsObject())
                    if (value.AsObject().TryGetPropertyValue(WireName(target,name),out var item)) result[name] = QueryModel(Text(member!["target"]),item,Text(member["format"]));
                return result;
            }
            case "list": return new JsonArray(value.AsArray().Select(item => QueryModel(Text(shape["member"]!["target"]),item,Text(shape["member"]!["format"]))).ToArray());
            case "map": {
                var result = new JsonObject(); foreach (var (key,item) in value.AsObject()) result[key] = QueryModel(Text(shape["value"]!["target"]),item,Text(shape["value"]!["format"])); return result;
            }
            default: return value.DeepClone();
        }
    }
    private static bool WaiterMatch(JsonNode matcher, JsonNode? input, JsonNode? output, Exception? error)
    {
        var kind = Text(matcher["kind"]);
        if (kind == "success") return (error is null) == Flag(matcher["expected"]);
        if (kind == "errorType") return error is ServiceError service && (service.ErrorName == Text(matcher["name"]) || service.ErrorName == Text(matcher["name"])!.Split('#')[^1]);
        if (error is not null) return false;
        var source = kind == "inputOutput" ? new JsonObject { ["input"] = input?.DeepClone(), ["output"] = output?.DeepClone() } : output;
        var value = Query.Evaluate(matcher["path"]!["expression"]!,source); var expected = Text(matcher["expected"]);
        bool IsString(JsonNode? node) => node?.GetValueKind() == JsonValueKind.String && Text(node) == expected;
        return Text(matcher["comparator"]) switch {
            "stringEquals" => IsString(value),
            "booleanEquals" => value?.GetValueKind() is JsonValueKind.True or JsonValueKind.False && value.GetValue<bool>() == (expected == "true"),
            "allStringEquals" => value is JsonArray all && all.Count != 0 && all.All(IsString),
            "anyStringEquals" => value is JsonArray any && any.Any(IsString),
            _ => throw new ArgumentException("Unknown waiter comparator")
        };
    }
    internal async Task<T?> WaitAsync<T>(string operation, string name, object? input, TimeSpan timeout, WaiterOptions? options, CancellationToken cancellationToken)
    {
        var op = schema["operations"]![operation]!; var waiter = op["waiters"]![name]!;
        var minimum = options?.MinDelay ?? TimeSpan.FromSeconds(waiter["minDelay"]!.GetValue<double>());
        var maximum = options?.MaxDelay ?? TimeSpan.FromSeconds(waiter["maxDelay"]!.GetValue<double>());
        if (timeout <= TimeSpan.Zero || timeout.TotalMilliseconds > uint.MaxValue - 1 || minimum < TimeSpan.Zero || maximum < minimum || maximum.TotalMilliseconds > uint.MaxValue - 1)
            throw new ArgumentOutOfRangeException(nameof(timeout),"Waiter deadline and ordered delays must be valid");
        var encoded = Encode(Text(op["input"]),input)?.AsObject() ?? new JsonObject();
        var queryInput = QueryModel(Text(op["input"]),encoded);
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken); deadline.CancelAfter(timeout);
        var clock = Stopwatch.StartNew(); int attempts = 0; T? response = default; Exception? error = null;
        try
        {
            while (true)
            {
                deadline.Token.ThrowIfCancellationRequested(); attempts++; error = null;
                try { response = await SendAsync<T>(Prepare(operation,(JsonObject)encoded.DeepClone()),deadline.Token).ConfigureAwait(false); }
                catch (Exception failure) when (failure is not OperationCanceledException) { response = default; error = failure; }
                var output = error is null ? QueryModel(Text(op["output"]),Encode(Text(op["output"]),response)) : null;
                string state = error is null ? "retry" : "failure"; string? message = null;
                foreach (var acceptor in waiter["acceptors"]!.AsArray())
                {
                    if (!WaiterMatch(acceptor!["matcher"]!,queryInput,output,error)) continue;
                    state = Text(acceptor["state"])!;
                    if (acceptor["message"] is JsonNode query)
                    {
                        var value = Query.Evaluate(query["expression"]!,output);
                        if (value?.GetValueKind() == JsonValueKind.String) message = Text(value);
                        else if (value is JsonArray list && list.All(item => item?.GetValueKind() == JsonValueKind.String)) message = string.Join(", ",list.Select(item => Text(item)));
                    }
                    break;
                }
                deadline.Token.ThrowIfCancellationRequested();
                if (state == "success") return response;
                if (state == "failure") throw new WaiterException(name,attempts,response,error,message);
                var remaining = timeout - clock.Elapsed;
                if (remaining <= TimeSpan.Zero) throw new WaiterTimeoutException(name,attempts,response,error);
                var ceiling = Math.Min(maximum.TotalMilliseconds,minimum.TotalMilliseconds * Math.Pow(2,Math.Min(attempts - 1,62)));
                var delay = TimeSpan.FromMilliseconds(minimum.TotalMilliseconds + Random.Shared.NextDouble() * (ceiling - minimum.TotalMilliseconds));
                if (remaining - delay <= minimum) delay = remaining;
                await Task.Delay(delay,deadline.Token).ConfigureAwait(false);
            }
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        { throw new WaiterTimeoutException(name,attempts,response,error); }
    }
}
