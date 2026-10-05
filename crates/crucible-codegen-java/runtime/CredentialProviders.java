import java.nio.file.Files;
import java.nio.file.Path;
import java.util.function.Supplier;

/** Explicit providers: no ambient identity lookup occurs unless configured. */
public final class CredentialProviders {
  private CredentialProviders() {}
  @SafeVarargs public static Supplier<AwsCredentials> chain(Supplier<AwsCredentials>... providers) {
    var snapshot=providers.clone();
    return () -> { for (var provider:snapshot) { var value=provider.get(); if (value!=null) return value; } return null; };
  }
  public static AwsCredentials environment() {
    String key=System.getenv("AWS_ACCESS_KEY_ID"), secret=System.getenv("AWS_SECRET_ACCESS_KEY");
    return key==null || key.isEmpty() || secret==null || secret.isEmpty() ? null : new AwsCredentials(key,secret,System.getenv("AWS_SESSION_TOKEN"),null);
  }
  public static Supplier<AwsCredentials> profile(Path path, String profile) {
    return () -> {
      if (!Files.exists(path)) return null;
      try {
        var values=new java.util.HashMap<String,String>(); boolean selected=false;
        for (String raw:Files.readAllLines(path)) {
          String line=raw.strip();
          if (line.isEmpty() || line.startsWith("#") || line.startsWith(";")) continue;
          if (line.startsWith("[") && line.endsWith("]")) { selected=line.substring(1,line.length()-1).strip().equals(profile); continue; }
          int split=line.indexOf('='); if (selected && split>=0) values.put(line.substring(0,split).strip(),line.substring(split+1).strip());
        }
        String key=values.get("aws_access_key_id"), secret=values.get("aws_secret_access_key");
        return key==null || secret==null ? null : new AwsCredentials(key,secret,values.get("aws_session_token"),null);
      } catch (java.io.IOException error) { throw new java.io.UncheckedIOException(error); }
    };
  }
}
