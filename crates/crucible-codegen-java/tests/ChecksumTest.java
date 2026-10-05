package example.checksums;

import com.sun.net.httpserver.HttpServer;
import example.checksums.model.Algorithm;
import example.checksums.model.Input;
import example.checksums.model.Validation;
import java.net.InetSocketAddress;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.atomic.AtomicReference;

public final class ChecksumTest {
  static void check(boolean ok,String message){if(!ok)throw new AssertionError(message);}
  // The bitwise reference is independent of the runtime's lookup-table update.
  static byte[] crc64Reference(byte[] bytes) {
    long value=-1L;
    for(byte item:bytes) {
      value^=Byte.toUnsignedLong(item);
      for(int bit=0;bit<8;bit++)value=(value>>>1)^((value&1)!=0?0x9a6c9329ac4bc9b5L:0L);
    }
    return ByteBuffer.allocate(8).putLong(~value).array();
  }
  static void crc64Incremental() {
    var random=new Random(137);
    for(int size:new int[]{0,1,2,7,8,9,255,256,257,4097}) {
      byte[] bytes=new byte[size];random.nextBytes(bytes);
      if(size==256)for(int index=0;index<size;index++)bytes[index]=(byte)index;
      byte[] expected=crc64Reference(bytes);
      check(Arrays.equals(Checksums.digest("CRC64NVME",bytes),expected),"CRC64 bitwise equivalence for "+size+" bytes");
      for(int kind=0;kind<3;kind++) {
        var buffer=kind==1?ByteBuffer.allocateDirect(size+16):ByteBuffer.allocate(size+16);
        buffer.position(5);buffer.put(bytes);buffer.flip();buffer.position(5);
        if(kind==2)buffer=buffer.asReadOnlyBuffer();
        var hasher=Checksums.hasher("CRC64NVME");
        hasher.update(buffer.slice().limit(0));
        while(buffer.hasRemaining()) {
          int count=Math.min(buffer.remaining(),1+random.nextInt(67));
          var chunk=buffer.slice().limit(count);
          hasher.update(chunk);
          check(chunk.position()==count && chunk.limit()==count,"CRC64 consumes exactly the chunk bounds");
          buffer.position(buffer.position()+count);
        }
        check(Arrays.equals(hasher.finish(),expected) && Arrays.equals(hasher.finish(),expected),"CRC64 chunked heap/direct/read-only equivalence");
      }
    }
  }
  public static void main(String[] args)throws Exception {
    crc64Incremental();
    var vectors=new LinkedHashMap<String,String>();
    vectors.put("CRC32","y/Q5Jg==");vectors.put("CRC32C","4waSgw==");vectors.put("CRC64NVME","rosUhgp5mIg=");
    vectors.put("MD5","JfnnlDI7RTiF9RgfG2JNCw==");vectors.put("SHA1","98O8HYCOBHMq32eZZczDTKeuNEE=");vectors.put("SHA256","FeKw08M4keuw8e9gnsQZQgwg4yDOlMZfvIwzEkSOsiU=");
    vectors.put("SHA512","2eZ2LdHI6vbWGzxhkvxAjU1tXxF20MKRabwk5xw/J0rSf81YEbMT1oH35V7ALXPUmclUVba1u1A6z1dPuo/+hQ==");
    var failure=new AtomicReference<Throwable>();var mode=new AtomicReference<String>("normal");var observed=new AtomicReference<com.sun.net.httpserver.Headers>();
    var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
    server.createContext("/",exchange->{try {
      byte[] bytes=exchange.getRequestBody().readAllBytes();String body=new String(bytes,StandardCharsets.UTF_8);observed.set(exchange.getRequestHeaders());
      String algorithm=exchange.getRequestHeaders().getFirst("x-algorithm");
      if(!mode.get().equals("skip")&&!mode.get().equals("explicit")) {
        String expected=body.isEmpty()?"1B2M2Y8AsgTpgAmY7PhCfg==":vectors.get(algorithm==null?"MD5":algorithm);
        String header=algorithm==null?"content-md5":"x-amz-checksum-"+algorithm.toLowerCase(Locale.ROOT);
        check(expected.equals(exchange.getRequestHeaders().getFirst(header)),"request checksum "+header+" "+expected);
      }
      String responseAlgorithm=algorithm==null?"CRC32":algorithm;
      if(!mode.get().equals("unmodeled"))exchange.getResponseHeaders().add("x-amz-checksum-"+responseAlgorithm.toLowerCase(Locale.ROOT),mode.get().equals("bad")?"AAAA":vectors.get(responseAlgorithm));
      exchange.getResponseHeaders().add("x-amz-checksum-xxhash3","invalid ignored checksum");
      if(mode.get().equals("one"))exchange.getResponseHeaders().add("x-amz-checksum-sha256","AAAA");
      byte[] response="123456789".getBytes(StandardCharsets.UTF_8);exchange.sendResponseHeaders(200,response.length);exchange.getResponseBody().write(response);
    }catch(Throwable error){failure.set(error);error.printStackTrace();exchange.sendResponseHeaders(500,-1);}finally{exchange.close();}});
    server.start();
    try {
      var config=new ClientConfig();config.endpoint="http://127.0.0.1:"+server.getAddress().getPort();config.maxAttempts=1;
      var validated=new ArrayList<String>();config.checksumValidated=(operation,algorithm)->validated.add(operation+":"+algorithm);
      var client=new ChecksumsClient(config);byte[] bytes="123456789".getBytes(StandardCharsets.UTF_8);
      client.legacy(b->b.data(bytes));client.required(b->b.data(bytes));client.legacy(b->b.data(new byte[0]));
      var input=Input.builder().data(bytes).build();client.flexible(input);
      check(!input.algorithm.present()&&!input.validation.present(),"input default mutation");
      check("CRC32".equals(observed.get().getFirst("x-algorithm"))&&"ENABLED".equals(observed.get().getFirst("x-validation")),"implicit defaults");
      check(validated.equals(List.of("Flexible:CRC32")),"validated callback");
      for(String algorithm:vectors.keySet()){client.flexible(b->b.data(bytes).algorithm(new Algorithm(algorithm)));check(validated.get(validated.size()-1).equals("Flexible:"+algorithm),"algorithm validation");}
      mode.set("bad");try{client.flexible(input);throw new AssertionError("corrupt response accepted");}catch(ChecksumMismatch expected){check(expected.algorithm.equals("CRC32"),"checksum error algorithm");}
      mode.set("one");client.flexible(input);
      mode.set("unmodeled");int count=validated.size();client.flexible(input);check(validated.size()==count,"unmodeled checksum ignored");
      mode.set("explicit");client.flexible(b->b.data(bytes).checksum("supplied"));check(observed.get().getFirst("x-amz-checksum-crc32").equals("supplied"),"explicit header preserved");
      mode.set("skip");config.requestChecksumCalculation=ChecksumPolicy.WHEN_REQUIRED;config.responseChecksumValidation=ChecksumPolicy.WHEN_REQUIRED;
      var optOut=new ChecksumsClient(config);optOut.flexible(input);check(observed.get().getFirst("x-algorithm")==null&&observed.get().getFirst("x-validation")==null&&observed.get().getFirst("x-amz-checksum-crc32")==null,"policy opt out");
      mode.set("normal");optOut.flexible(b->b.data(bytes).algorithm(Algorithm.SHA256).validation(Validation.ENABLED));
      try{client.flexible(b->b.data(bytes).algorithm(Algorithm.XXHASH64));throw new AssertionError("unsupported algorithm accepted");}catch(IllegalArgumentException expected){}
      config.middleware.add(request->request.body=bytes);new ChecksumsClient(config).legacy(b->b.data("changed".getBytes(StandardCharsets.UTF_8)));
      if(failure.get()!=null)throw new AssertionError(failure.get());
    }finally{server.stop(0);}
    System.out.println("Java checksums passed: seven known vectors, defaults, override, policies, corruption, callback");
  }
}
