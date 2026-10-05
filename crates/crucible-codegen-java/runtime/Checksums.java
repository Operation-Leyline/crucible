import com.google.gson.JsonObject;
import java.net.http.HttpHeaders;
import java.nio.ByteBuffer;
import java.security.MessageDigest;
import java.util.Base64;
import java.util.List;
import java.util.Locale;
import java.util.Map;

final class Checksums {
  private static final List<String> SUPPORTED=List.of("CRC32","CRC32C","CRC64NVME","MD5","SHA1","SHA256","SHA512");
  record Plan(String operation,String algorithm,boolean legacy,boolean validate,JsonObject descriptor) {}
  private static String text(JsonObject object,String key) { var value=object.get(key);return value==null||value.isJsonNull()?null:value.getAsString(); }
  // A stream is never buffered to compute a header, so only an explicit or required
  // algorithm applies to it, and its header must then be supplied precomputed.
  static Plan prepare(String operation,JsonObject op,JsonObject values,ClientConfig config,boolean streaming) {
    var checksum=op.getAsJsonObject("checksum");if(checksum==null)return new Plan(operation,null,false,false,null);
    if("md5".equals(text(checksum,"kind")))return new Plan(operation,"MD5",true,false,checksum);
    String member=text(checksum,"requestAlgorithmMember"),algorithm=member==null?null:text(values,member);
    boolean required=checksum.has("requestRequired")&&checksum.get("requestRequired").getAsBoolean();
    boolean legacy=member==null&&required;
    if(legacy)algorithm="MD5";
    else if(member!=null&&algorithm==null&&(required||!streaming&&config.requestChecksumCalculation==ChecksumPolicy.WHEN_SUPPORTED)) {
      algorithm="CRC32";values.addProperty(member,algorithm);
    }
    if(algorithm!=null) {
      if(!SUPPORTED.contains(algorithm))throw new IllegalArgumentException("Unsupported checksum algorithm: "+algorithm);
    }
    String mode=text(checksum,"validationModeMember");
    if(mode!=null&&text(values,mode)==null&&config.responseChecksumValidation==ChecksumPolicy.WHEN_SUPPORTED)values.addProperty(mode,"ENABLED");
    boolean validate=mode!=null && "ENABLED".equals(text(values,mode));
    return new Plan(operation,algorithm,legacy,validate,checksum);
  }
  static void request(Plan plan,Map<String,String> headers,byte[] body,boolean streaming) {
    if(plan.algorithm()==null)return;
    String name=plan.legacy()?"content-md5":header(plan.algorithm());
    if(headers.keySet().stream().anyMatch(key->key.equalsIgnoreCase(name)))return;
    if(streaming)throw new IllegalArgumentException("streaming request checksum "+name+" must be supplied precomputed");
    headers.put(name,Base64.getEncoder().encodeToString(digest(plan.algorithm(),body==null?new byte[0]:body)));
  }
  static void validate(Plan plan,HttpHeaders headers,byte[] body,ClientConfig config) {
    if(!plan.validate()||plan.descriptor()==null||!plan.descriptor().has("responseAlgorithms"))return;
    for(var item:plan.descriptor().getAsJsonArray("responseAlgorithms")) {
      String algorithm=item.getAsString();if(!SUPPORTED.contains(algorithm))continue;
      String supplied=headers.firstValue(header(algorithm)).orElse(null);if(supplied==null)continue;
      byte[] expected;try{expected=Base64.getDecoder().decode(supplied);}catch(IllegalArgumentException invalid){throw new ChecksumMismatch(algorithm);}
      if(!MessageDigest.isEqual(expected,digest(algorithm,body)))throw new ChecksumMismatch(algorithm);
      if(config.checksumValidated!=null)config.checksumValidated.accept(plan.operation(),algorithm);
      return;
    }
  }
  /** Verifies a streaming response as it is read; null when nothing is to be verified. */
  static Running running(Plan plan,HttpHeaders headers,ClientConfig config) {
    if(!plan.validate()||plan.descriptor()==null||!plan.descriptor().has("responseAlgorithms"))return null;
    for(var item:plan.descriptor().getAsJsonArray("responseAlgorithms")) {
      String algorithm=item.getAsString();if(!SUPPORTED.contains(algorithm))continue;
      String supplied=headers.firstValue(header(algorithm)).orElse(null);if(supplied==null)continue;
      byte[] expected;try{expected=Base64.getDecoder().decode(supplied);}catch(IllegalArgumentException invalid){expected=null;}
      return new Running(plan.operation(),algorithm,expected,hasher(algorithm),config.checksumValidated);
    }
    return null;
  }
  static final class Running {
    private final String operation,algorithm;private final byte[] expected;private final Hasher hasher;
    private final java.util.function.BiConsumer<String,String> validated;
    Running(String operation,String algorithm,byte[] expected,Hasher hasher,java.util.function.BiConsumer<String,String> validated){
      this.operation=operation;this.algorithm=algorithm;this.expected=expected;this.hasher=hasher;this.validated=validated;
    }
    void update(ByteBuffer bytes){hasher.update(bytes);}
    void verify(){
      if(expected==null||!MessageDigest.isEqual(expected,hasher.finish()))throw new ChecksumMismatch(algorithm);
      if(validated!=null)validated.accept(operation,algorithm);
    }
  }
  interface Hasher { void update(ByteBuffer bytes); byte[] finish(); }
  static Hasher hasher(String algorithm) {
    if(algorithm.equals("CRC32")||algorithm.equals("CRC32C")) {
      java.util.zip.Checksum crc=algorithm.equals("CRC32")?new java.util.zip.CRC32():new java.util.zip.CRC32C();
      return new Hasher(){
        public void update(ByteBuffer bytes){crc.update(bytes);}
        public byte[] finish(){long value=crc.getValue();return new byte[]{(byte)(value>>>24),(byte)(value>>>16),(byte)(value>>>8),(byte)value};}
      };
    }
    if(algorithm.equals("CRC64NVME")) return new Hasher(){
      long value=-1L;
      public void update(ByteBuffer bytes){while(bytes.hasRemaining()){value^=Byte.toUnsignedLong(bytes.get());for(int bit=0;bit<8;bit++)value=(value>>>1)^((value&1)!=0?0x9a6c9329ac4bc9b5L:0L);}}
      public byte[] finish(){long result=~value;byte[] output=new byte[8];for(int index=0;index<8;index++)output[index]=(byte)(result>>>(56-index*8));return output;}
    };
    MessageDigest digest;
    try{digest=MessageDigest.getInstance(switch(algorithm){case "SHA1"->"SHA-1";case "SHA256"->"SHA-256";case "SHA512"->"SHA-512";case "MD5"->"MD5";default->throw new IllegalArgumentException("Unsupported checksum algorithm: "+algorithm);});}
    catch(java.security.NoSuchAlgorithmException impossible){throw new IllegalStateException(impossible);}
    return new Hasher(){
      public void update(ByteBuffer bytes){digest.update(bytes);}
      public byte[] finish(){return digest.digest();}
    };
  }
  private static String header(String algorithm){return "x-amz-checksum-"+algorithm.toLowerCase(Locale.ROOT);}
  static byte[] digest(String algorithm,byte[] bytes) {
    var hasher=hasher(algorithm);hasher.update(ByteBuffer.wrap(bytes));return hasher.finish();
  }
}
