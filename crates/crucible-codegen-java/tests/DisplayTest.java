package example.docs;

import com.google.gson.JsonPrimitive;
import example.docs.model.Choice;
import example.docs.model.GreetInput;
import example.docs.model.Mode;
import example.docs.model.Profile;
import example.docs.model.Unavailable;
import java.util.List;

/** Generated toString output is readable and never prints a sensitive value. */
public final class DisplayTest {
  static final String HIDDEN = "*** Sensitive Data Redacted ***";

  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }

  static void equal(String expected, Object actual) {
    check(expected.equals(String.valueOf(actual)), "expected <" + expected + "> but was <" + actual + ">");
  }

  public static void main(String[] args) {
    var profile = Profile.builder().nickname("ada").pin("1234").build();
    var input = GreetInput.builder().name("Ada").password("hunter2").token("tok-1")
        .secrets(List.of("s3cret", "other")).names(List.of("a", "b")).profile(profile)
        .choice(Choice.builder().code("c0de").build()).mode(Mode.FAST).build();
    String text = input.toString();
    for (String secret : List.of("hunter2", "tok-1", "s3cret", "other", "1234", "c0de"))
      check(!text.contains(secret), "leaked " + secret + " in " + text);
    equal("GreetInput(name=Ada, password=" + HIDDEN + ", token=" + HIDDEN + ", secrets=" + HIDDEN
        + ", names=[a, b], profile=Profile(nickname=ada, pin=" + HIDDEN + "), choice=Choice(code=" + HIDDEN
        + "), mode=fast)", text);

    equal("GreetInput(name=x)", GreetInput.builder().name("x").build());
    equal("Profile(nickname=null)", Profile.builder().nickname(null).build());
    equal("Choice(label=hi)", Choice.builder().label("hi").build());
    equal("Choice(unknownVariant=UnknownUnion(name=future, value=" + HIDDEN + "))",
        Choice.builder().unknownVariant(new UnknownUnion("future", new JsonPrimitive("secret"))).build());

    var error = Unavailable.builder().message("down").retryToken("retry-me").build();
    equal("Unavailable(message=down, retryToken=" + HIDDEN + ")", error);
    check(!error.toString().contains("retry-me"), "error leaked its token");
  }
}
