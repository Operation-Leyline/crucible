import 'dart:io';
Map<String,String> environment() => Platform.environment;
String? readProfile(String path) { final file = File(path); return file.existsSync() ? file.readAsStringSync() : null; }
