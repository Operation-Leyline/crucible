package example.query;

import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;

/** Independent consumer of the generated awsQuery and ec2Query clients, over real HTTP. */
public final class QueryProtocolTest {
  record Reply(int status, Map<String, String> headers, String body) {}

  interface Handler { Reply handle(Map<String, String> form, String raw) throws Exception; }

  static void check(boolean condition, String message) { if (!condition) throw new AssertionError(message); }

  static final AtomicReference<Handler> NEXT = new AtomicReference<>();
  static final AtomicReference<Throwable> FAILURE = new AtomicReference<>();

  static Map<String, String> form(String raw) {
    Map<String, String> result = new LinkedHashMap<>();
    for (String pair : raw.split("&")) {
      String[] parts = pair.split("=", 2);
      result.put(URLDecoder.decode(parts[0], StandardCharsets.UTF_8), parts.length > 1 ? URLDecoder.decode(parts[1], StandardCharsets.UTF_8) : null);
    }
    return result;
  }

  public static void main(String[] args) throws Exception {
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        check(exchange.getRequestMethod().equals("POST") && exchange.getRequestURI().getPath().equals("/"), "query route");
        check(exchange.getRequestHeaders().getFirst("content-type").equals("application/x-www-form-urlencoded"), "form content type");
        String raw = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
        Reply reply = NEXT.get().handle(form(raw), raw);
        reply.headers().forEach((key, value) -> exchange.getResponseHeaders().add(key, value));
        byte[] bytes = reply.body().getBytes(StandardCharsets.UTF_8);
        exchange.sendResponseHeaders(reply.status(), bytes.length == 0 ? -1 : bytes.length);
        if (bytes.length > 0) exchange.getResponseBody().write(bytes);
      } catch (Throwable error) {
        FAILURE.compareAndSet(null, error);
        exchange.sendResponseHeaders(599, -1);
      } finally {
        exchange.close();
      }
    });
    server.start();
    try {
      String endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
      query(endpoint);
      ec2(endpoint);
      if (FAILURE.get() != null) throw new AssertionError(FAILURE.get());
    } finally {
      server.stop(0);
    }
    System.out.println("Java generated awsQuery and ec2Query clients passed");
  }

  static void query(String endpoint) {
    var config = new ClientConfig();
    config.endpoint = endpoint;
    config.maxAttempts = 1;
    config.headers.put("Content-Type", "text/plain");
    var client = new AwsQueryClient(config);
    NEXT.set((form, raw) -> {
      check(raw.startsWith("Action=QueryMaps&Version=2020-01-08&"), "action and version lead the form: " + raw);
      check(form.equals(Map.of("Action", "QueryMaps", "Version", "2020-01-08",
          "MapArg.entry.1.key", "k", "MapArg.entry.1.value", "v w",
          "Hi.1.K", "a", "Hi.1.V", "A",
          "MapOfLists.entry.1.key", "l", "MapOfLists.entry.1.value.member.1", "x")), "awsQuery maps: " + raw);
      check(raw.contains("v%20w"), "RFC 3986 spaces");
      return new Reply(200, Map.of(), "<QueryMapsResponse xmlns=\"https://example.com/\"><QueryMapsResult/></QueryMapsResponse>");
    });
    client.queryMaps(b -> b.MapArg(Map.of("k", "v w")).FlattenedMapWithXmlName(Map.of("a", "A")).MapOfLists(Map.of("l", List.of("x"))));
    NEXT.set((form, raw) -> {
      check(form.equals(Map.of("Action", "QueryLists", "Version", "2020-01-08", "ListArg", "",
          "ComplexListArg.member.1.hi", "h", "FlattenedListArg.1", "f", "ListArgWithXmlNameMember.item.1", "i")), "awsQuery lists: " + raw);
      return new Reply(200, Map.of(), "");
    });
    client.queryLists(b -> b.ListArg(List.of()).ComplexListArg(List.of(example.query.model.GreetingStruct.builder().hi("h").build()))
        .FlattenedListArg(List.of("f")).ListArgWithXmlNameMember(List.of("i")));
    NEXT.set((form, raw) -> {
      check(raw.equals("Action=QueryTimestamps&Version=2020-01-08&normalFormat=2015-01-25T08%3A00%3A00.500Z&epochMember=1422172800.5"), "timestamps: " + raw);
      return new Reply(200, Map.of(), "");
    });
    client.queryTimestamps(b -> b.normalFormat(Instant.ofEpochSecond(1422172800, 500_000_000)).epochMember(Instant.ofEpochSecond(1422172800, 500_000_000)));
    NEXT.set((form, raw) -> new Reply(200, Map.of("Content-Type", "text/xml"),
        "<SimpleScalarXmlPropertiesResponse xmlns=\"https://example.com/\"><SimpleScalarXmlPropertiesResult>"
        + "<stringValue>s</stringValue><emptyStringValue/><integerValue>3</integerValue><floatValue>-Infinity</floatValue>"
        + "</SimpleScalarXmlPropertiesResult><ResponseMetadata><RequestId>r</RequestId></ResponseMetadata></SimpleScalarXmlPropertiesResponse>"));
    var scalars = client.simpleScalarXmlProperties();
    check(scalars.stringValue().equals("s") && scalars.emptyStringValue().isEmpty() && scalars.integerValue() == 3, "awsQuery result wrapper");
    check(scalars.floatValue() == Float.NEGATIVE_INFINITY && !scalars.longValue.present(), "awsQuery scalars");
    NEXT.set((form, raw) -> new Reply(402, Map.of(),
        "<ErrorResponse><Error><Type>Sender</Type><Code>Customized</Code><Message>Hi</Message></Error><RequestId>foo-id</RequestId></ErrorResponse>"));
    try { client.greetingWithErrors(); throw new AssertionError("custom code error expected"); }
    catch (example.query.model.CustomCodeError error) {
      check(error.errorName().equals("Customized") && error.Message().equals("Hi") && error.requestId().equals("foo-id") && error.status() == 402, "awsQueryError code");
    }
    NEXT.set((form, raw) -> new Reply(400, Map.of("x-amzn-RequestId", "header-id"),
        "<ErrorResponse><Error><Code>ComplexError</Code><TopLevel>t</TopLevel><Nested><Foo>bar</Foo></Nested></Error><RequestId>foo-id</RequestId></ErrorResponse>"));
    try { client.greetingWithErrors(); throw new AssertionError("complex error expected"); }
    catch (example.query.model.ComplexError error) {
      check(error.TopLevel().equals("t") && error.Nested().Foo().equals("bar") && error.requestId().equals("header-id"), "awsQuery complex error");
    }
  }

  static void ec2(String endpoint) {
    var config = new example.ec2.ClientConfig();
    config.endpoint = endpoint;
    config.maxAttempts = 1;
    var client = new example.ec2.AwsEc2Client(config);
    NEXT.set((form, raw) -> {
      check(form.equals(Map.of("Action", "SimpleInputParams", "Version", "2020-01-08", "LowerName", "l",
          "UsesXmlName", "u", "HasQueryAndXmlName", "q", "Qux", "dmFsdWU=", "Boo", "NaN")), "ec2 names: " + raw);
      return new Reply(200, Map.of(), "");
    });
    client.simpleInputParams(b -> b.lowerName("l").UsesXmlName("u").HasQueryAndXmlName("q")
        .Qux("value".getBytes(StandardCharsets.UTF_8)).Boo(Double.NaN));
    NEXT.set((form, raw) -> {
      check(form.equals(Map.of("Action", "QueryLists", "Version", "2020-01-08",
          "ComplexListArg.1.Hi", "h", "Hi.1", "a", "Hi.2", "b", "NestedWithList.ListArg.1", "n")), "ec2 lists: " + raw);
      return new Reply(200, Map.of(), "");
    });
    client.queryLists(b -> b.ListArg(List.of()).ComplexListArg(List.of(example.ec2.model.GreetingStruct.builder().hi("h").build()))
        .ListArgWithXmlName(List.of("a", "b")).NestedWithList(example.ec2.model.NestedStructWithList.builder().ListArg(List.of("n")).build()));
    NEXT.set((form, raw) -> new Reply(200, Map.of(),
        "<XmlListsResponse xmlns=\"https://example.com/\"><stringList><member>a</member></stringList>"
        + "<flattenedList>x</flattenedList><flattenedList>y</flattenedList><requestId>r</requestId></XmlListsResponse>"));
    var lists = client.xmlLists();
    check(lists.stringList().equals(List.of("a")) && lists.flattenedList().equals(List.of("x", "y")), "ec2 response without wrapper");
    NEXT.set((form, raw) -> new Reply(400, Map.of(),
        "<Response><Errors><Error><Code>InvalidGreeting</Code><Message>Hi</Message></Error></Errors><RequestID>foo-id</RequestID></Response>"));
    try { client.greetingWithErrors(); throw new AssertionError("ec2 error expected"); }
    catch (example.ec2.model.InvalidGreeting error) {
      check(error.Message().equals("Hi") && error.requestId().equals("foo-id") && error.errorName().equals("InvalidGreeting"), "ec2 error envelope");
    }
  }
}
