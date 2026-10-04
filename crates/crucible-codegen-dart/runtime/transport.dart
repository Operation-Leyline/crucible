import 'dart:async';
import 'dart:typed_data';
import 'package:http/http.dart' as http;
import 'cancellation.dart';

final class HttpRequest {
  final Uri url;
  final String method;
  final Map<String, String> headers;
  final Uint8List? body;
  HttpRequest(
      {required this.url,
      required this.method,
      Map<String, String> headers = const {},
      Uint8List? body})
      : headers = Map.unmodifiable(headers),
        body = body == null ? null : Uint8List.fromList(body);
  HttpRequest copyWith(
          {Uri? url,
          String? method,
          Map<String, String>? headers,
          Uint8List? body}) =>
      HttpRequest(
          url: url ?? this.url,
          method: method ?? this.method,
          headers: headers ?? this.headers,
          body: body ?? this.body);
}

final class HttpResponse {
  final int status;
  final Map<String, String> headers;
  final Uint8List body;
  HttpResponse(
      {required this.status,
      Map<String, String> headers = const {},
      Uint8List? body})
      : headers = Map.unmodifiable({
          for (final entry in headers.entries)
            entry.key.toLowerCase(): entry.value
        }),
        body = body ?? Uint8List(0);
}

abstract interface class HttpTransport {
  Future<HttpResponse> send(HttpRequest request,
      {required bool readBody, required CancellationToken cancellation});
  void close();
}

/// Uses package:http's native or browser client. Redirects are disabled.
/// A supplied http.Client remains caller-owned.
final class DefaultHttpTransport implements HttpTransport {
  final http.Client _client;
  final bool _owned;
  DefaultHttpTransport({http.Client? client})
      : _client = client ?? http.Client(),
        _owned = client == null;
  @override
  Future<HttpResponse> send(HttpRequest request,
      {required bool readBody, required CancellationToken cancellation}) async {
    cancellation.throwIfCancelled();
    final aborted = Completer<void>();
    final unlink = cancellation.listen(() {
      if (!aborted.isCompleted) aborted.complete();
    });
    try {
      final native = http.AbortableRequest(request.method, request.url,
          abortTrigger: aborted.future)
        ..followRedirects = false;
      if (request.body != null) native.bodyBytes = request.body!;
      native.headers.addAll(request.headers);
      // BrowserClient uses Fetch's default cache mode. Request directives both
      // bypass existing entries and prevent credential-dependent responses from
      // being stored. Browser servers must allow Cache-Control in CORS headers.
      native.headers['cache-control'] = 'no-cache, no-store';
      final response = await _client.send(native);
      cancellation.throwIfCancelled();
      if (response.statusCode >= 200 &&
          response.statusCode < 300 &&
          !readBody) {
        // Cancel the stream rather than draining an irrelevant or stalled body.
        await response.stream.listen(null).cancel();
        return HttpResponse(
            status: response.statusCode, headers: response.headers);
      }
      final bytes = await response.stream.toBytes();
      cancellation.throwIfCancelled();
      return HttpResponse(
          status: response.statusCode, headers: response.headers, body: bytes);
    } on http.RequestAbortedException {
      cancellation.throwIfCancelled();
      rethrow;
    } finally {
      unlink();
    }
  }

  @override
  void close() {
    if (_owned) _client.close();
  }
}
