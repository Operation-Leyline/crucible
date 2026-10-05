package example.xml;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import example.xml.model.ComplexError;
import example.xml.model.FooEnum;
import example.xml.model.InvalidGreeting;
import example.xml.model.PayloadWithXmlName;
import example.xml.model.StructureListMember;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;

/** Independent consumer of the generated restXml client, over real HTTP. */
public final class XmlProtocolTest {
  record Reply(int status, Map<String, String> headers, String body) {}

  interface Handler { Reply handle(HttpExchange exchange, String body) throws Exception; }

  static void check(boolean condition, String message) { if (!condition) throw new AssertionError(message); }

  static final AtomicReference<Handler> NEXT = new AtomicReference<>();
  static final AtomicReference<Throwable> FAILURE = new AtomicReference<>();

  public static void main(String[] args) throws Exception {
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        String body = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
        Reply reply = NEXT.get().handle(exchange, body);
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
      var config = new ClientConfig();
      config.endpoint = endpoint;
      config.maxAttempts = 1;
      var client = new RestXmlClient(config);
      requests(client);
      responses(client);
      errors(client, endpoint);
      hostile(client);
      if (FAILURE.get() != null) throw new AssertionError(FAILURE.get());
    } finally {
      server.stop(0);
    }
    System.out.println("Java generated restXml client passed");
  }

  static void requests(RestXmlClient client) {
    Instant time = Instant.ofEpochSecond(1398796238);
    NEXT.set((exchange, body) -> {
      check(exchange.getRequestMethod().equals("PUT") && exchange.getRequestURI().getPath().equals("/XmlLists"), "restXml route");
      check(exchange.getRequestHeaders().getFirst("content-type").equals("application/xml"), "restXml content type");
      check(body.equals("<XmlListsRequest><stringList><member>a &amp; b</member></stringList>"
          + "<timestampList><member>2014-04-29T18:30:38Z</member></timestampList><enumList><member>Foo</member></enumList>"
          + "<nestedStringList><member><member>x</member></member></nestedStringList><renamed><item>r</item></renamed>"
          + "<flattenedList>hi</flattenedList><flattenedList>bye</flattenedList>"
          + "<flattenedListWithMemberNamespace xmlns=\"https://xml-member.example.com\">n</flattenedListWithMemberNamespace>"
          + "<myStructureList><item><value>1</value><other>2</other></item></myStructureList></XmlListsRequest>"), "restXml list body: " + body);
      return new Reply(200, Map.of("content-type", "application/xml"), "<XmlListsResponse/>");
    });
    var lists = client.xmlLists(b -> b.stringList(List.of("a & b")).timestampList(List.of(time)).enumList(List.of(FooEnum.FOO))
        .nestedStringList(List.of(List.of("x"))).renamedListMembers(List.of("r")).flattenedList(List.of("hi", "bye"))
        .flattenedListWithMemberNamespace(List.of("n"))
        .structureList(List.of(StructureListMember.builder().a("1").b("2").build())));
    check(!lists.stringList.present(), "absent list stays absent");

    NEXT.set((exchange, body) -> {
      check(exchange.getRequestHeaders().getFirst("x-foo").equals("Foo"), "header binding stays a header");
      check(body.equals("<SimpleScalarPropertiesRequest><stringValue>\r\n</stringValue><trueBooleanValue>true</trueBooleanValue>"
          .replace("\r", "&#xD;") + "<floatValue>NaN</floatValue><DoubleDribble>-Infinity</DoubleDribble></SimpleScalarPropertiesRequest>"),
          "scalar body: " + body);
      return new Reply(200, Map.of("X-Foo", "Bar"), "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<SimpleScalarPropertiesResponse>"
          + "<stringValue>a&#xD;&#10;<![CDATA[<b>]]></stringValue><byteValue> 7 </byteValue><floatValue>Infinity</floatValue>"
          + "<DoubleDribble>2.5</DoubleDribble></SimpleScalarPropertiesResponse>");
    });
    var scalars = client.simpleScalarProperties(b -> b.foo("Foo").stringValue("\r\n").trueBooleanValue(true)
        .floatValue(Float.NaN).doubleValue(Double.NEGATIVE_INFINITY));
    check(scalars.foo().equals("Bar") && scalars.stringValue().equals("a\r\n<b>") && scalars.byteValue() == 7, "scalar response");
    check(scalars.floatValue() == Float.POSITIVE_INFINITY && scalars.doubleValue() == 2.5, "non-finite response floats");

    NEXT.set((exchange, body) -> {
      check(body.equals("<Hola><name>Phreddy</name></Hola>"), "payload member name: " + body);
      return new Reply(200, Map.of(), "<Anything><name>Back</name></Anything>");
    });
    var payload = client.httpPayloadWithMemberXmlName(b -> b.nested(PayloadWithXmlName.builder().name("Phreddy").build()));
    check(payload.nested().name().equals("Back"), "payload response root name is not checked");

    NEXT.set((exchange, body) -> {
      check(body.equals("<XmlAttributesRequest test=\"&lt;&quot;&amp;&#xA;\"><foo>x</foo></XmlAttributesRequest>"), "attribute escaping: " + body);
      return new Reply(200, Map.of(), "<XmlAttributesResponse test=\"a&amp;b\"><foo>x</foo></XmlAttributesResponse>");
    });
    check(client.xmlAttributes(b -> b.foo("x").attr("<\"&\n")).attr().equals("a&b"), "attribute response");

    NEXT.set((exchange, body) -> {
      check(body.isEmpty() && exchange.getRequestHeaders().getFirst("content-type") == null, "no body for an empty input");
      return new Reply(200, Map.of(), "");
    });
    client.noInputAndOutput();
  }

  static void responses(RestXmlClient client) {
    NEXT.set((exchange, body) -> new Reply(200, Map.of(), "<XmlMapsResponse><myMap>"
        + "<entry><key>b</key><value><hi>2</hi></value></entry><entry><key>a</key><value><hi>1</hi></value></entry>"
        + "</myMap></XmlMapsResponse>"));
    var maps = client.xmlMaps();
    check(maps.myMap().get("a").hi().equals("1") && maps.myMap().get("b").hi().equals("2"), "map response");

    NEXT.set((exchange, body) -> new Reply(200, Map.of(), "<r><myMap><key>k</key><value>Baz</value></myMap><myMap><key>j</key><value>0</value></myMap></r>"));
    var flat = client.flattenedXmlMap();
    check(flat.myMap().get("k").equals(FooEnum.BAZ) && flat.myMap().get("j").equals(FooEnum.ZERO), "flattened map response");

    NEXT.set((exchange, body) -> new Reply(200, Map.of(), "<x:XmlTimestampsResponse xmlns:x=\"urn:x\"><x:epochSeconds>1.5</x:epochSeconds>"
        + "<x:httpDate>Tue, 29 Apr 2014 18:30:38 GMT</x:httpDate><x:normal>2014-04-29T18:30:38.123Z</x:normal></x:XmlTimestampsResponse>"));
    var times = client.xmlTimestamps();
    check(times.epochSeconds().equals(Instant.ofEpochSecond(1, 500_000_000)), "epoch seconds response");
    check(times.httpDate().equals(Instant.ofEpochSecond(1398796238)) && times.normal().toEpochMilli() == 1398796238123L, "text timestamps");

    NEXT.set((exchange, body) -> new Reply(200, Map.of(), "<XmlBlobsResponse><data>dmFsdWU=</data></XmlBlobsResponse>"));
    check(new String(client.xmlBlobs().data(), StandardCharsets.UTF_8).equals("value"), "blob response");
  }

  static void errors(RestXmlClient client, String endpoint) {
    NEXT.set((exchange, body) -> new Reply(400, Map.of("content-type", "application/xml"),
        "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidGreeting</Code><Message>Hi</Message></Error><RequestId>foo-id</RequestId></ErrorResponse>"));
    try { client.greetingWithErrors(); throw new AssertionError("modeled error expected"); }
    catch (InvalidGreeting error) {
      check(error.Message().equals("Hi") && error.getMessage().equals("Hi") && error.errorName().equals("InvalidGreeting"), "wrapped error");
      check(error.requestId().equals("foo-id") && error.status() == 400, "wrapped error metadata");
    }
    NEXT.set((exchange, body) -> new Reply(403, Map.of("X-Header", "Header", "x-amz-request-id", "header-id"),
        "<ErrorResponse><Error><Code>ComplexError</Code><TopLevel>Top</TopLevel><Nested><Foo>bar</Foo></Nested></Error><RequestId>body-id</RequestId></ErrorResponse>"));
    try { client.greetingWithErrors(); throw new AssertionError("complex error expected"); }
    catch (ComplexError error) {
      check(error.Header().equals("Header") && error.TopLevel().equals("Top") && error.Nested().Foo().equals("bar"), "complex error members");
      check(error.requestId().equals("header-id"), "request id header wins over the body");
    }
    NEXT.set((exchange, body) -> new Reply(503, Map.of(), "<ErrorResponse><Error><Code>SlowDown</Code><Message>later</Message></Error></ErrorResponse>"));
    try { client.greetingWithErrors(); throw new AssertionError("unmodeled error expected"); }
    catch (ServiceError error) {
      check(error.getClass() == ServiceError.class && error.errorName().equals("SlowDown") && error.getMessage().equals("later"), "unmodeled code");
    }
    // S3-style services send the <Error> element without the ErrorResponse wrapper.
    var config = new example.nowrap.ClientConfig();
    config.endpoint = endpoint;
    config.maxAttempts = 1;
    var unwrapped = new example.nowrap.RestXmlNoErrorWrappingClient(config);
    NEXT.set((exchange, body) -> new Reply(400, Map.of(),
        "<Error><Type>Sender</Type><Code>InvalidGreeting</Code><Message>Hi</Message><RequestId>foo-id</RequestId></Error>"));
    try { unwrapped.greetingWithErrors(); throw new AssertionError("unwrapped error expected"); }
    catch (example.nowrap.model.InvalidGreeting error) {
      check(error.Message().equals("Hi") && error.requestId().equals("foo-id"), "unwrapped error");
    }
  }

  static void hostile(RestXmlClient client) {
    // External entities and entity expansion are refused before any are resolved.
    for (String body : List.of(
        "<?xml version=\"1.0\"?><!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><SimpleScalarPropertiesResponse><stringValue>&x;</stringValue></SimpleScalarPropertiesResponse>",
        "<!DOCTYPE r [<!ENTITY a \"aaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]><SimpleScalarPropertiesResponse><stringValue>&b;</stringValue></SimpleScalarPropertiesResponse>",
        "<SimpleScalarPropertiesResponse><stringValue>&undeclared;</stringValue></SimpleScalarPropertiesResponse>",
        "<SimpleScalarPropertiesResponse><stringValue>open</stringValue>",
        "<SimpleScalarPropertiesResponse test=\"&undeclared;\"/>",
        "<a/><b/>")) {
      NEXT.set((exchange, request) -> new Reply(200, Map.of(), body));
      try { client.simpleScalarProperties(); throw new AssertionError("hostile XML accepted: " + body); }
      catch (IllegalArgumentException expected) {}
    }
    NEXT.set((exchange, request) -> new Reply(200, Map.of(), "<r>" + "<a>".repeat(300) + "</a>".repeat(300) + "</r>"));
    try { client.xmlBlobs(); throw new AssertionError("deep XML accepted"); }
    catch (IllegalArgumentException expected) {}
    // A hostile error body still yields the HTTP metadata.
    NEXT.set((exchange, request) -> new Reply(400, Map.of(), "<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><Error><Code>&x;</Code></Error>"));
    try { client.greetingWithErrors(); throw new AssertionError("error expected"); }
    catch (ServiceError error) {
      // Without a readable code the status selects the one 400 error; no entity was resolved.
      check(error.status() == 400 && !error.errorName().contains("root"), "hostile error body is not resolved");
    }
    try {
      client.simpleScalarProperties(b -> b.stringValue("bad \u0001 control"));
      throw new AssertionError("unrepresentable text accepted");
    } catch (IllegalArgumentException expected) {}
  }
}
