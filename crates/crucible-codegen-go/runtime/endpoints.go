package sdk

import (
    "encoding/json"
    "errors"
    "net"
    "net/url"
    "regexp"
    "strconv"
    "strings"
)

var endpointLabel = regexp.MustCompile(`^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$`)
func copyEndpointParameters(input map[string]any) (map[string]any,error) {
    result := map[string]any{}
    for name,value := range input {
        switch item := value.(type) {
        case nil,string,bool: result[name] = item
        case []string: values := make([]any,len(item)); for i,text := range item { values[i] = text }; result[name] = values
        case []any: values := append([]any{},item...); for _,entry := range values { if _,ok := entry.(string); !ok { return nil,errors.New("endpoint parameter arrays require strings") } }; result[name] = values
        default: return nil,errors.New("endpoint parameters require strings, booleans, or string arrays")
        }
    }; return result,nil
}
func hostLabel(text string, subdomains bool) bool {
    labels := []string{text}; if subdomains { labels = strings.Split(text,".") }
    for _, label := range labels { if !endpointLabel.MatchString(label) { return false } }; return true
}
func parseEndpoint(text string) map[string]any {
    if strings.ContainsAny(text,"?#") { return nil }; for _,ch := range text { if ch <= 32 || ch == 127 { return nil } }
    uri,err := url.Parse(text)
    if err != nil || uri.User != nil || uri.Host == "" || (uri.Scheme != "http" && uri.Scheme != "https") { return nil }
    host := uri.Hostname(); ip := net.ParseIP(host) != nil
    if !ip && !hostLabel(strings.TrimSuffix(host,"."),true) { return nil }
    authority := strings.ToLower(host); if strings.Contains(authority,":") { authority = "["+authority+"]" }
    port := uri.Port(); if port != "" { number,err := strconv.Atoi(port); if err != nil || number > 65535 || number < 0 { return nil }; if !((uri.Scheme == "https" && number == 443) || (uri.Scheme == "http" && number == 80)) { authority += ":"+port } }
    path := uri.EscapedPath(); if path == "" { path = "/" }; normalized := path; if !strings.HasSuffix(normalized,"/") { normalized += "/" }
    return map[string]any{"scheme":uri.Scheme,"authority":authority,"path":path,"normalizedPath":normalized,"isIp":ip}
}
func endpointAttribute(value, path any) any {
    var steps []any
    if text,ok := path.(string); ok {
        for _,part := range strings.Split(text,".") {
            key,index,found := strings.Cut(part,"[")
            if key != "" { steps = append(steps,map[string]any{"kind":"member","name":key}) }
            if found { number,err := strconv.Atoi(strings.TrimSuffix(index,"]")); if err != nil { qfail("invalid endpoint attribute index") }; steps = append(steps,map[string]any{"kind":"index","index":json.Number(strconv.Itoa(number))}) }
        }
    } else { steps = qarray(path) }
    for _,item := range steps {
        step := qobject(item)
        if step["kind"] == "member" { if object,ok := value.(map[string]any); ok { value = object[qtext(step["name"])] } else { return nil } } else {
            list,ok := value.([]any); if !ok { return nil }; index := qinteger(step["index"]); if index < 0 { index += len(list) }; if index < 0 || index >= len(list) { return nil }; value = list[index]
        }
        if value == nil { return nil }
    }; return value
}
func endpointFunction(name string, args []any) any {
    boolean := func(value any)bool { if v,ok := value.(bool); ok { return v }; qfail("endpoint function requires boolean"); return false }
    switch name {
    case "isSet": return args[0] != nil
    case "not": return !boolean(args[0])
    case "booleanEquals": a,ok := args[0].(bool); b,ok2 := args[1].(bool); return ok && ok2 && a == b
    case "stringEquals": a,ok := args[0].(string); b,ok2 := args[1].(string); return ok && ok2 && a == b
    case "getAttr": return endpointAttribute(args[0],args[1])
    case "substring":
        text := qtext(args[0]); start,end := qinteger(args[1]),qinteger(args[2]); for _,ch := range text { if ch > 127 { return nil } }; if start < 0 || end < start || end > len(text) { return nil }; if boolean(args[3]) { return text[len(text)-end:len(text)-start] }; return text[start:end]
    case "isValidHostLabel": return hostLabel(qtext(args[0]),boolean(args[1]))
    case "parseUrl": result := parseEndpoint(qtext(args[0])); if result == nil { return nil }; return result
    case "uriEncode": return escape(qtext(args[0]))
    }; qfail("unknown endpoint function"); return nil
}
func endpointExpression(node map[string]any, values map[string]any) any {
    eval := func(value any) any { return endpointExpression(qobject(value),values) }
    switch node["kind"] {
    case "reference": return values[qtext(node["name"])]
    case "boolean","integer": return node["value"]
    case "array": result := []any{}; for _,item := range qarray(node["values"]) { result = append(result,eval(item)) }; return result
    case "object": result := map[string]any{}; for key,item := range qobject(node["values"]) { result[key] = eval(item) }; return result
    case "call": args := []any{}; for _,item := range qarray(node["arguments"]) { args = append(args,eval(item)) }; return endpointFunction(qtext(node["function"]),args)
    case "string":
        var text strings.Builder
        for _,item := range qarray(node["parts"]) {
            part := qobject(item); value := part["value"]
            if part["kind"] != "literal" { value = endpointAttribute(values[qtext(part["name"])],part["path"]) }
            switch v := value.(type) { case string: text.WriteString(v); case bool: text.WriteString(strconv.FormatBool(v)); case json.Number: text.WriteString(string(v)); default: qfail("endpoint template references missing or non-scalar value") }
        }; return text.String()
    }; qfail("unknown endpoint expression"); return nil
}
type resolvedEndpoint struct { URL string; Headers map[string][]string; Properties map[string]any }
func endpointRules(nodes []any, parameters map[string]any) *resolvedEndpoint {
    for _,entry := range nodes {
        rule := qobject(entry); scope := map[string]any{}; for key,value := range parameters { scope[key] = value }; matched := true
        for _,item := range qarray(rule["conditions"]) {
            condition := qobject(item); args := []any{}; for _,arg := range qarray(condition["arguments"]) { args = append(args,endpointExpression(qobject(arg),scope)) }
            value := endpointFunction(qtext(condition["function"]),args)
            if value == nil { matched = false; break }; if boolean,ok := value.(bool); ok && !boolean { matched = false; break }
            if assign,ok := condition["assign"].(string); ok { scope[assign] = value }
        }
        if !matched { continue }; result := qobject(rule["result"])
        switch result["kind"] {
        case "tree": if nested := endpointRules(qarray(result["rules"]),scope); nested != nil { return nested }
        case "error": qfail(qtext(endpointExpression(qobject(result["message"]),scope)))
        case "endpoint":
            endpoint := qtext(endpointExpression(qobject(result["url"]),scope)); if parseEndpoint(endpoint) == nil { qfail("endpoint rule returned invalid URL") }
            headers := map[string][]string{}; properties := map[string]any{}
            for key,items := range qobject(result["headers"]) { for _,item := range qarray(items) { headers[key] = append(headers[key],qtext(endpointExpression(qobject(item),scope))) } }
            for key,item := range qobject(result["properties"]) { properties[key] = endpointExpression(qobject(item),scope) }
            return &resolvedEndpoint{URL:endpoint,Headers:headers,Properties:properties}
        }
    }; return nil
}
func resolveEndpoint(rules map[string]any, sources map[string][]map[string]any, input any, client, builtins map[string]any) (result *resolvedEndpoint, err error) {
    defer func(){ if failure := recover(); failure != nil { if value,ok := failure.(queryFailure); ok { err = errors.New(value.message) } else { panic(failure) } } }()
    parameters := map[string]any{}
    for name,item := range qobject(rules["parameters"]) {
        parameter := qobject(item); var value any
        for _,source := range sources[name] {
            switch source["kind"] {
            case "static","default": value = source["value"]
            case "inputMember": if object,ok := input.(map[string]any); ok { value = object[qtext(source["member"])] }
            case "inputPath": value = qeval(qobject(qobject(source["path"])["expression"]),input)
            case "client": value = client[qtext(source["name"])]
            case "builtIn": value = builtins[qtext(source["name"])]
            }; if value != nil { break }
        }
        if value == nil && parameter["required"] == true { qfail("required endpoint parameter missing: "+name) }
        if value != nil {
            valid := false
            switch parameter["kind"] { case "string": _,valid = value.(string); case "boolean": _,valid = value.(bool); case "stringArray": if list,ok := value.([]any); ok { valid = true; for _,item := range list { if _,ok := item.(string); !ok { valid = false } } } }
            if !valid { qfail("wrong endpoint parameter type: "+name) }
        }
        parameters[name] = value
    }
    result = endpointRules(qarray(rules["rules"]),parameters); if result == nil { return nil,errors.New("no endpoint rule matched") }; return result,nil
}
func prefixEndpoint(endpoint string, description map[string]any, input any) (string,error) {
    prefix := ""; object,_ := input.(map[string]any)
    for _,item := range description["parts"].([]any) {
        part := item.(map[string]any)
        if part["kind"] == "literal" { prefix += part["value"].(string) } else { value,ok := object[part["member"].(string)].(string); if !ok || !hostLabel(value,false) { return "",errors.New("invalid endpoint host label") }; prefix += value }
    }
    uri,err := url.Parse(endpoint); if err != nil { return "",err }; host := prefix+uri.Hostname()
    if len(host) > 253 || !hostLabel(host,true) { return "",errors.New("invalid expanded endpoint hostname") }
    if port := uri.Port(); port != "" { host += ":"+port }; uri.Host = host; return uri.String(),nil
}
