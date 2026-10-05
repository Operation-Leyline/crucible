import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.io.ByteArrayInputStream;
import java.math.BigDecimal;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import javax.xml.stream.XMLInputFactory;
import javax.xml.stream.XMLStreamConstants;
import javax.xml.stream.XMLStreamException;
import javax.xml.stream.XMLStreamReader;

/**
 * The AWS XML family: restXml documents, awsQuery and ec2Query forms, and the XML
 * responses and error envelopes of all three. Values are the runtime's JSON wire values,
 * keyed by member name; element and query names come from the lowered {@code xmlShapes}.
 */
final class XmlProtocol {
  private static final Set<String> PROTOCOLS = Set.of("awsRestXml", "awsQuery", "ec2Query");
  private static final int MAX_DEPTH = 256;
  private final JsonObject shapes, xmlShapes, serviceNamespace, operations;
  private final String protocol, version;

  static boolean handles(String protocol) {
    return protocol != null && PROTOCOLS.contains(protocol);
  }

  XmlProtocol(JsonObject schema) {
    shapes = schema.getAsJsonObject("shapes");
    xmlShapes = schema.has("xmlShapes") ? schema.getAsJsonObject("xmlShapes") : new JsonObject();
    serviceNamespace = schema.has("xmlNamespace") ? schema.getAsJsonObject("xmlNamespace") : null;
    operations = schema.has("operations") ? schema.getAsJsonObject("operations") : new JsonObject();
    protocol = schema.get("protocol").getAsString();
    version = schema.get("version").getAsString();
  }

  boolean query() {
    return !"awsRestXml".equals(protocol);
  }

  // ---------------------------------------------------------------- schema access

  private static String text(JsonObject object, String key) {
    JsonElement value = object == null ? null : object.get(key);
    return value == null || value.isJsonNull() ? null : value.getAsString();
  }

  private static boolean flag(JsonObject object, String key) {
    return object != null && object.has(key) && object.get(key).getAsBoolean();
  }

  private JsonObject shape(String target) {
    return shapes.getAsJsonObject(target);
  }

  private JsonObject xmlShape(String target) {
    JsonObject shape = xmlShapes.getAsJsonObject(target);
    if (shape == null) throw new IllegalStateException("no XML description for " + target);
    return shape;
  }

  private JsonObject xmlMember(String owner, String member) {
    return xmlShape(owner).getAsJsonObject("members").getAsJsonObject(member);
  }

  /** The member description of a structure member, or of a list or map's member/key/value. */
  private JsonObject member(String owner, String member) {
    JsonObject shape = shape(owner);
    return shape.has("members") ? shape.getAsJsonObject("members").getAsJsonObject(member) : shape.getAsJsonObject(member);
  }

  private static JsonObject namespace(JsonObject xml) {
    JsonElement value = xml == null ? null : xml.get("namespace");
    return value == null || value.isJsonNull() ? null : value.getAsJsonObject();
  }

  // ---------------------------------------------------------------- XML writing

  /** A restXml body: the input's body members under its root element. */
  byte[] document(String target, Collection<String> members, JsonObject values) {
    JsonObject xml = xmlShape(target);
    JsonObject namespace = namespace(xml) != null ? namespace(xml) : serviceNamespace;
    StringBuilder out = new StringBuilder();
    out.append('<').append(text(xml, "name"));
    declare(out, namespace);
    attributes(out, target, values, members);
    out.append('>');
    for (String member : members) child(out, target, member, values.get(member));
    out.append("</").append(text(xml, "name")).append('>');
    return out.toString().getBytes(StandardCharsets.UTF_8);
  }

  /** A restXml structure or union payload, named by the member's resolved payload name. */
  byte[] payload(String owner, String member, JsonElement value) {
    JsonObject xml = xmlMember(owner, member);
    JsonObject namespace = namespace(xml) != null ? namespace(xml) : serviceNamespace;
    StringBuilder out = new StringBuilder();
    element(out, text(xml, "payloadName"), namespace, text(member(owner, member), "target"), value);
    return out.toString().getBytes(StandardCharsets.UTF_8);
  }

  private static void declare(StringBuilder out, JsonObject namespace) {
    if (namespace == null) return;
    String prefix = text(namespace, "prefix");
    out.append(prefix == null ? " xmlns" : " xmlns:" + prefix).append("=\"").append(escape(text(namespace, "uri"), true)).append('"');
  }

  private void attributes(StringBuilder out, String target, JsonObject value, Collection<String> members) {
    for (String member : members) {
      JsonObject xml = xmlMember(target, member);
      JsonElement item = value.get(member);
      if (flag(xml, "attribute") && item != null && !item.isJsonNull())
        out.append(' ').append(text(xml, "name")).append("=\"").append(escape(scalar(item), true)).append('"');
    }
  }

  private void element(StringBuilder out, String name, JsonObject namespace, String target, JsonElement value) {
    JsonObject shape = shape(target);
    out.append('<').append(name);
    declare(out, namespace);
    switch (text(shape, "kind")) {
      case "structure", "union" -> {
        JsonObject object = value.getAsJsonObject();
        var members = shape.getAsJsonObject("members").keySet();
        attributes(out, target, object, members);
        out.append('>');
        for (String member : members) child(out, target, member, object.get(member));
      }
      case "list" -> {
        out.append('>');
        JsonObject item = xmlMember(target, "member");
        String itemTarget = text(shape.getAsJsonObject("member"), "target");
        for (JsonElement entry : value.getAsJsonArray()) element(out, text(item, "name"), namespace(item), itemTarget, entry);
      }
      case "map" -> {
        out.append('>');
        for (var entry : value.getAsJsonObject().entrySet()) {
          out.append("<entry>");
          entry(out, target, entry.getKey(), entry.getValue());
          out.append("</entry>");
        }
      }
      default -> out.append('>').append(escape(scalar(value), false));
    }
    out.append("</").append(name).append('>');
  }

  private void entry(StringBuilder out, String map, String key, JsonElement value) {
    JsonObject shape = shape(map), keyXml = xmlMember(map, "key"), valueXml = xmlMember(map, "value");
    element(out, text(keyXml, "name"), namespace(keyXml), text(shape.getAsJsonObject("key"), "target"), new JsonPrimitive(key));
    element(out, text(valueXml, "name"), namespace(valueXml), text(shape.getAsJsonObject("value"), "target"), value);
  }

  private void child(StringBuilder out, String owner, String member, JsonElement value) {
    if (value == null || value.isJsonNull()) return;
    JsonObject xml = xmlMember(owner, member);
    if (flag(xml, "attribute")) return;
    String target = text(member(owner, member), "target");
    if (!flag(xml, "flattened")) {
      element(out, text(xml, "name"), namespace(xml), target, value);
      return;
    }
    // A flattened collection repeats the member's own element, without a wrapper.
    JsonObject shape = shape(target);
    if ("list".equals(text(shape, "kind"))) {
      JsonObject item = xmlMember(target, "member");
      JsonObject namespace = namespace(xml) != null ? namespace(xml) : namespace(item);
      for (JsonElement entry : value.getAsJsonArray())
        element(out, text(xml, "name"), namespace, text(shape.getAsJsonObject("member"), "target"), entry);
      return;
    }
    for (var entry : value.getAsJsonObject().entrySet()) {
      out.append('<').append(text(xml, "name"));
      declare(out, namespace(xml));
      out.append('>');
      entry(out, target, entry.getKey(), entry.getValue());
      out.append("</").append(text(xml, "name")).append('>');
    }
  }

  /** Scalar wire text: numbers without exponent noise, everything else as its JSON string. */
  private static String scalar(JsonElement value) {
    if (value.isJsonPrimitive() && value.getAsJsonPrimitive().isNumber()) {
      Number number = value.getAsNumber();
      if (number instanceof BigDecimal decimal)
        return decimal.signum() == 0 ? "0" : decimal.stripTrailingZeros().toPlainString();
      return number.toString();
    }
    if (!value.isJsonPrimitive()) throw new IllegalArgumentException("expected a scalar XML value");
    return value.getAsString();
  }

  static String escape(String text, boolean attribute) {
    StringBuilder out = new StringBuilder(text.length());
    for (int i = 0; i < text.length(); ) {
      int c = text.codePointAt(i);
      boolean valid = c == 0x9 || c == 0xA || c == 0xD || (c >= 0x20 && c <= 0xD7FF) || (c >= 0xE000 && c <= 0xFFFD) || (c >= 0x10000 && c <= 0x10FFFF);
      if (!valid) throw new IllegalArgumentException("text contains a character XML 1.0 cannot carry");
      switch (c) {
        case '&' -> out.append("&amp;");
        case '<' -> out.append("&lt;");
        case '>' -> out.append("&gt;");
        // Character references survive end-of-line and attribute normalization.
        case '\r' -> out.append("&#xD;");
        case '"' -> out.append(attribute ? "&quot;" : "\"");
        case '\n' -> out.append(attribute ? "&#xA;" : "\n");
        case '\t' -> out.append(attribute ? "&#x9;" : "\t");
        default -> out.appendCodePoint(c);
      }
      i += Character.charCount(c);
    }
    return out.toString();
  }

  // ---------------------------------------------------------------- query forms

  /** An awsQuery or ec2Query request body. */
  byte[] form(String operation, String target, JsonObject values) {
    List<String> pairs = new ArrayList<>();
    pairs.add("Action=" + formEscape(operation));
    pairs.add("Version=" + formEscape(version));
    if (target != null && "structure".equals(text(shape(target), "kind"))) members(pairs, "", target, values);
    return String.join("&", pairs).getBytes(StandardCharsets.UTF_8);
  }

  private void members(List<String> pairs, String prefix, String target, JsonObject values) {
    boolean ec2 = "ec2Query".equals(protocol);
    for (String member : shape(target).getAsJsonObject("members").keySet()) {
      JsonElement value = values.get(member);
      if (value == null || value.isJsonNull()) continue;
      JsonObject xml = xmlMember(target, member);
      String name = text(xml, ec2 ? "queryName" : "name");
      formValue(pairs, prefix.isEmpty() ? name : prefix + "." + name, text(member(target, member), "target"), value, flag(xml, "flattened"));
    }
  }

  private void formValue(List<String> pairs, String key, String target, JsonElement value, boolean flattened) {
    JsonObject shape = shape(target);
    // ec2Query always flattens collections and ignores their member names.
    boolean ec2 = "ec2Query".equals(protocol), flat = flattened || ec2;
    switch (text(shape, "kind")) {
      case "structure", "union" -> members(pairs, key, target, value.getAsJsonObject());
      case "list" -> {
        JsonArray items = value.getAsJsonArray();
        // awsQuery spells an empty list as the bare key; ec2Query leaves it out.
        if (items.isEmpty() && !ec2) pairs.add(formEscape(key) + "=");
        String itemName = text(xmlMember(target, "member"), "name");
        String itemTarget = text(shape.getAsJsonObject("member"), "target");
        for (int i = 0; i < items.size(); i++)
          formValue(pairs, key + (flat ? "." : "." + itemName + ".") + (i + 1), itemTarget, items.get(i), false);
      }
      case "map" -> {
        String keyName = text(xmlMember(target, "key"), "name"), valueName = text(xmlMember(target, "value"), "name");
        int index = 1;
        for (var entry : value.getAsJsonObject().entrySet()) {
          String base = key + (flat ? "." : ".entry.") + index++;
          formValue(pairs, base + "." + keyName, text(shape.getAsJsonObject("key"), "target"), new JsonPrimitive(entry.getKey()), false);
          formValue(pairs, base + "." + valueName, text(shape.getAsJsonObject("value"), "target"), entry.getValue(), false);
        }
      }
      default -> pairs.add(formEscape(key) + "=" + formEscape(scalar(value)));
    }
  }

  private static String formEscape(String text) {
    return URLEncoder.encode(text, StandardCharsets.UTF_8).replace("+", "%20").replace("%7E", "~").replace("*", "%2A");
  }

  // ---------------------------------------------------------------- XML reading

  /** A parsed element; names are qualified as written, so {@code prefix:local} or {@code local}. */
  static final class Node {
    final String name, local;
    final Map<String, String> attributes = new LinkedHashMap<>();
    final List<Node> children = new ArrayList<>();
    final StringBuilder text = new StringBuilder();

    Node(String name, String local) {
      this.name = name;
      this.local = local;
    }

    boolean is(String expected) {
      return name.equals(expected) || (expected.indexOf(':') < 0 && local.equals(expected));
    }

    Node child(String expected) {
      for (Node child : children) if (child.is(expected)) return child;
      return null;
    }

    List<Node> all(String expected) {
      List<Node> result = new ArrayList<>();
      for (Node child : children) if (child.is(expected)) result.add(child);
      return result;
    }

    String attribute(String expected) {
      String value = attributes.get(expected);
      if (value != null || expected.indexOf(':') >= 0) return value;
      for (var entry : attributes.entrySet())
        if (entry.getKey().substring(entry.getKey().indexOf(':') + 1).equals(expected)) return entry.getValue();
      return null;
    }

    Node find(String expected) {
      if (is(expected)) return this;
      for (Node child : children) {
        Node found = child.find(expected);
        if (found != null) return found;
      }
      return null;
    }
  }

  /** Parses untrusted XML: no DTDs, no entity expansion beyond the predefined five. */
  static Node parse(byte[] bytes) {
    XMLInputFactory factory = XMLInputFactory.newDefaultFactory();
    factory.setProperty(XMLInputFactory.SUPPORT_DTD, false);
    factory.setProperty(XMLInputFactory.IS_SUPPORTING_EXTERNAL_ENTITIES, false);
    factory.setProperty(XMLInputFactory.IS_NAMESPACE_AWARE, true);
    factory.setProperty(XMLInputFactory.IS_COALESCING, true);
    XMLStreamReader reader = null;
    try {
      reader = factory.createXMLStreamReader(new ByteArrayInputStream(bytes));
      ArrayDeque<Node> stack = new ArrayDeque<>();
      Node root = null;
      while (reader.hasNext()) {
        switch (reader.next()) {
          case XMLStreamConstants.DTD, XMLStreamConstants.ENTITY_REFERENCE, XMLStreamConstants.ENTITY_DECLARATION ->
              throw new IllegalArgumentException("XML document type declarations and entities are not accepted");
          case XMLStreamConstants.START_ELEMENT -> {
            if (root != null) throw new IllegalArgumentException("XML document has several root elements");
            if (stack.size() >= MAX_DEPTH) throw new IllegalArgumentException("XML nesting limit");
            Node node = new Node(qualified(reader.getPrefix(), reader.getLocalName()), reader.getLocalName());
            for (int i = 0; i < reader.getAttributeCount(); i++)
              node.attributes.put(qualified(reader.getAttributePrefix(i), reader.getAttributeLocalName(i)), reader.getAttributeValue(i));
            if (!stack.isEmpty()) stack.peek().children.add(node);
            stack.push(node);
          }
          case XMLStreamConstants.CHARACTERS, XMLStreamConstants.CDATA, XMLStreamConstants.SPACE -> {
            if (!stack.isEmpty()) stack.peek().text.append(reader.getText());
          }
          case XMLStreamConstants.END_ELEMENT -> {
            Node done = stack.pop();
            if (stack.isEmpty()) root = done;
          }
          default -> {}
        }
      }
      if (root == null) throw new IllegalArgumentException("XML document has no root element");
      return root;
    } catch (XMLStreamException error) {
      throw new IllegalArgumentException("malformed XML: " + error.getMessage(), error);
    } finally {
      if (reader != null) try { reader.close(); } catch (XMLStreamException ignored) {}
    }
  }

  private static String qualified(String prefix, String local) {
    return prefix == null || prefix.isEmpty() ? local : prefix + ":" + local;
  }

  /** The output members of a successful response; {@code only} limits them to the body. */
  JsonObject output(String operation, String target, Collection<String> only, String payload, byte[] body) {
    JsonObject values = new JsonObject();
    if (target == null) return values;
    if (payload != null) {
      if (body.length == 0) return values;
      String payloadTarget = text(member(target, payload), "target");
      values.add(payload, read(parse(body), payloadTarget, null));
      return values;
    }
    if (only.isEmpty() || blank(body)) return values;
    Node root = parse(body);
    if (flag(operations.getAsJsonObject(operation), "unwrappedXmlOutput")) {
      String name = only.iterator().next();
      if (!root.is(text(xmlMember(target, name), "name")))
        throw new IllegalArgumentException("unwrapped XML response root differs from its body member");
      JsonObject description = member(target, name);
      values.add(name, read(root, text(description, "target"), text(description, "format")));
      return values;
    }
    if ("awsQuery".equals(protocol)) {
      // awsQuery wraps the members in <OperationResult>, beside <ResponseMetadata>.
      root = root.child(operation + "Result");
      if (root == null) return values;
    }
    return members(root, target, only);
  }

  private static boolean blank(byte[] body) {
    for (byte b : body) if (b != ' ' && b != '\t' && b != '\r' && b != '\n') return false;
    return true;
  }

  record ErrorEnvelope(String code, String message, String requestId, Node error) {}

  /**
   * Finds the error in any of the family's envelopes: {@code <ErrorResponse><Error>},
   * restXml's unwrapped {@code <Error>}, or ec2Query's {@code <Response><Errors><Error>}.
   */
  static ErrorEnvelope error(byte[] body) {
    Node root;
    try {
      root = parse(body);
    } catch (IllegalArgumentException malformed) {
      return new ErrorEnvelope(null, null, null, null);
    }
    Node error = root.find("Error");
    Node id = root.find("RequestId");
    if (id == null) id = root.find("RequestID");
    return new ErrorEnvelope(
        error == null ? null : textOf(error.child("Code")),
        error == null ? null : textOf(error.child("Message")),
        textOf(id),
        error);
  }

  private static String textOf(Node node) {
    return node == null ? null : node.text.toString().strip();
  }

  JsonObject members(Node node, String target, Collection<String> only) {
    JsonObject result = new JsonObject();
    for (String member : only) {
      JsonObject xml = xmlMember(target, member), description = member(target, member);
      String memberTarget = text(description, "target"), format = text(description, "format"), name = text(xml, "name");
      if (flag(xml, "attribute")) {
        String value = node.attribute(name);
        if (value != null) result.add(member, scalar(memberTarget, value, format));
      } else if (flag(xml, "flattened")) {
        List<Node> found = node.all(name);
        if (!found.isEmpty()) result.add(member, collection(found, memberTarget));
      } else {
        Node child = node.child(name);
        if (child != null) result.add(member, read(child, memberTarget, format));
      }
    }
    return result;
  }

  /** The flattened elements of a list or map, each one an item or an entry. */
  private JsonElement collection(List<Node> nodes, String target) {
    JsonObject shape = shape(target);
    if ("list".equals(text(shape, "kind"))) {
      JsonArray result = new JsonArray();
      JsonObject item = shape.getAsJsonObject("member");
      for (Node node : nodes) result.add(read(node, text(item, "target"), text(item, "format")));
      return result;
    }
    JsonObject result = new JsonObject();
    for (Node node : nodes) entry(result, node, target);
    return result;
  }

  private void entry(JsonObject result, Node node, String map) {
    JsonObject shape = shape(map);
    Node key = node.child(text(xmlMember(map, "key"), "name")), value = node.child(text(xmlMember(map, "value"), "name"));
    if (key == null || value == null) throw new IllegalArgumentException("XML map entry requires a key and a value");
    JsonObject valueMember = shape.getAsJsonObject("value");
    result.add(key.text.toString(), read(value, text(valueMember, "target"), text(valueMember, "format")));
  }

  private JsonElement read(Node node, String target, String format) {
    JsonObject shape = shape(target);
    switch (text(shape, "kind")) {
      case "structure", "union": {
        JsonObject result = members(node, target, shape.getAsJsonObject("members").keySet());
        if ("union".equals(text(shape, "kind")) && result.size() == 0) {
          // An undeclared variant is kept by name, as its text.
          for (Node child : node.children) {
            result.add(child.name, new JsonPrimitive(child.text.toString()));
            break;
          }
        }
        return result;
      }
      case "list": {
        JsonArray result = new JsonArray();
        JsonObject item = shape.getAsJsonObject("member");
        for (Node child : node.all(text(xmlMember(target, "member"), "name")))
          result.add(read(child, text(item, "target"), text(item, "format")));
        return result;
      }
      case "map": {
        JsonObject result = new JsonObject();
        for (Node child : node.all("entry")) entry(result, child, target);
        return result;
      }
      default:
        return scalar(target, node.text.toString(), format);
    }
  }

  private JsonElement scalar(String target, String text, String format) {
    JsonObject shape = shape(target);
    String trimmed = text.strip();
    switch (text(shape, "kind")) {
      case "string", "enum":
        return new JsonPrimitive(text);
      case "boolean":
        if (!trimmed.equals("true") && !trimmed.equals("false")) throw new IllegalArgumentException("expected XML boolean");
        return new JsonPrimitive(Boolean.parseBoolean(trimmed));
      case "float", "double":
        if (Set.of("NaN", "Infinity", "-Infinity").contains(trimmed)) return new JsonPrimitive(trimmed);
        return new JsonPrimitive(new BigDecimal(trimmed));
      case "byte", "short", "integer", "long", "bigInteger", "bigDecimal", "intEnum":
        return new JsonPrimitive(new BigDecimal(trimmed));
      case "timestamp":
        String effective = format == null ? text(shape, "format") : format;
        return "epoch-seconds".equals(effective) ? new JsonPrimitive(new BigDecimal(trimmed)) : new JsonPrimitive(trimmed);
      case "blob":
        return new JsonPrimitive(trimmed);
      default:
        throw new IllegalArgumentException("XML cannot carry " + text(shape, "kind"));
    }
  }
}
