import com.google.gson.JsonObject;
import java.net.http.HttpHeaders;
import java.security.MessageDigest;
import java.util.Base64;
import java.util.List;
import java.util.Locale;
import java.util.Map;

final class Checksums {
  private static final List<String> SUPPORTED=List.of("CRC32","CRC32C","CRC64NVME","MD5","SHA1","SHA256","SHA512");
  record Plan(String operation,String algorithm,boolean legacy,boolean validate,JsonObject descriptor) {}
  private static String text(JsonObject object,String key) { var value=object.get(key);return value==null||value.isJsonNull()?null:value.getAsString(); }
  static Plan prepare(String operation,JsonObject op,JsonObject values,ClientConfig config) {
    var checksum=op.getAsJsonObject("checksum");if(checksum==null)return new Plan(operation,null,false,false,null);
    if("md5".equals(text(checksum,"kind")))return new Plan(operation,"MD5",true,false,checksum);
    String member=text(checksum,"requestAlgorithmMember"),algorithm=member==null?null:text(values,member);
    boolean required=checksum.has("requestRequired")&&checksum.get("requestRequired").getAsBoolean();
    boolean legacy=member==null&&required;
    if(legacy)algorithm="MD5";
    else if(member!=null&&algorithm==null&&(required||config.requestChecksumCalculation==ChecksumPolicy.WHEN_SUPPORTED)) {
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
  static void request(Plan plan,Map<String,String> headers,byte[] body) {
    if(plan.algorithm()==null)return;
    String name=plan.legacy()?"content-md5":header(plan.algorithm());
    if(headers.keySet().stream().anyMatch(key->key.equalsIgnoreCase(name)))return;
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
  private static String header(String algorithm){return "x-amz-checksum-"+algorithm.toLowerCase(Locale.ROOT);}
  static byte[] digest(String algorithm,byte[] bytes) {
    if(algorithm.equals("CRC32")||algorithm.equals("CRC32C")) {
      java.util.zip.Checksum crc=algorithm.equals("CRC32")?new java.util.zip.CRC32():new java.util.zip.CRC32C();crc.update(bytes,0,bytes.length);long value=crc.getValue();
      return new byte[]{(byte)(value>>>24),(byte)(value>>>16),(byte)(value>>>8),(byte)value};
    }
    if(algorithm.equals("CRC64NVME")) {
      long value=-1L;
      for(byte item:bytes){value^=Byte.toUnsignedLong(item);for(int bit=0;bit<8;bit++)value=(value>>>1)^((value&1)!=0?0x9a6c9329ac4bc9b5L:0L);}
      value=~value;byte[] output=new byte[8];for(int index=0;index<8;index++)output[index]=(byte)(value>>>(56-index*8));return output;
    }
    try{return MessageDigest.getInstance(switch(algorithm){case "SHA1"->"SHA-1";case "SHA256"->"SHA-256";case "SHA512"->"SHA-512";case "MD5"->"MD5";default->throw new IllegalArgumentException("Unsupported checksum algorithm: "+algorithm);}).digest(bytes);}
    catch(java.security.NoSuchAlgorithmException impossible){throw new IllegalStateException(impossible);}
  }
}
