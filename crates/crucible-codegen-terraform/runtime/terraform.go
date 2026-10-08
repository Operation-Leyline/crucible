package provider

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"os"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/provider"
	pschema "github.com/hashicorp/terraform-plugin-framework/provider/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/boolplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/float64planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/int64planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/tfsdk"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

type infraProperty struct {
	Kind                           string
	Required, Immutable, Sensitive bool
	Description                    string
	Enum                           []string
	MinLength, MaxLength           *uint64
	Min, Max                       *json.Number
}
type infraOperation struct {
	Name          string
	Input, Output map[string][]string
}
type infraResourceDescription struct {
	Name           string
	Properties     map[string]infraProperty
	Operations     map[string]infraOperation
	NotFoundErrors []string
}

var infraDescriptions = func() []infraResourceDescription {
	var result []infraResourceDescription
	if err := json.Unmarshal([]byte(__DESCRIPTORS__), &result); err != nil {
		panic(err)
	}
	return result
}()

type infraProvider struct{}

func New() provider.Provider { return &infraProvider{} }
func (*infraProvider) Metadata(_ context.Context, _ provider.MetadataRequest, resp *provider.MetadataResponse) {
	resp.TypeName = __PROVIDER_NAME__
	resp.Version = "0.1.0"
}
func (*infraProvider) Schema(_ context.Context, _ provider.SchemaRequest, resp *provider.SchemaResponse) {
	resp.Schema = pschema.Schema{Attributes: map[string]pschema.Attribute{
		"endpoint":     pschema.StringAttribute{Required: true, Description: "Service endpoint. Credentials require HTTPS except loopback during testing."},
		"bearer_token": pschema.StringAttribute{Optional: true, Sensitive: true, Description: "Bearer credential. Prefer CRUCIBLE_TOKEN so credentials are not written into configuration."},
	}}
}
func (*infraProvider) Configure(ctx context.Context, req provider.ConfigureRequest, resp *provider.ConfigureResponse) {
	var endpoint, token types.String
	resp.Diagnostics.Append(req.Config.GetAttribute(ctx, path.Root("endpoint"), &endpoint)...)
	resp.Diagnostics.Append(req.Config.GetAttribute(ctx, path.Root("bearer_token"), &token)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if endpoint.IsUnknown() || endpoint.IsNull() || endpoint.ValueString() == "" || token.IsUnknown() {
		resp.Diagnostics.AddError("Provider configuration unavailable", "endpoint and bearer_token must be known before configuring resources.")
		return
	}
	secret := token.ValueString()
	if token.IsNull() {
		secret = os.Getenv("CRUCIBLE_TOKEN")
	}
	// A deployment retry is not necessarily an API-safe retry. Never replay a
	// lifecycle mutation automatically, even when the service client permits it.
	client, err := newClient(Config{Endpoint: endpoint.ValueString(), MaxAttempts: 1, Token: func(context.Context) (string, error) { return secret, nil }}, wireSchema, false)
	if err != nil {
		resp.Diagnostics.AddError("Invalid provider configuration", "The service endpoint is invalid.")
		return
	}
	resp.ResourceData = client
}
func (*infraProvider) DataSources(context.Context) []func() datasource.DataSource { return nil }
func (*infraProvider) Resources(context.Context) []func() resource.Resource {
	result := []func() resource.Resource{}
	for _, desc := range infraDescriptions {
		result = append(result, func() resource.Resource { return &infraResource{desc: desc} })
	}
	return result
}

type infraResource struct {
	desc   infraResourceDescription
	client *clientRuntime
}

var _ resource.ResourceWithConfigure = (*infraResource)(nil)
var _ resource.ResourceWithImportState = (*infraResource)(nil)
var _ resource.ResourceWithValidateConfig = (*infraResource)(nil)

func (r *infraResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_" + r.desc.Name
}
func (r *infraResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = r.schema()
}
func (r *infraResource) schema() schema.Schema {
	attrs := map[string]schema.Attribute{"id": schema.StringAttribute{Computed: true, PlanModifiers: []planmodifier.String{stringplanmodifier.UseStateForUnknown()}}}
	for name, p := range r.desc.Properties {
		switch p.Kind {
		case "string":
			a := schema.StringAttribute{Required: p.Required, Optional: !p.Required, Sensitive: p.Sensitive, Description: p.Description}
			if p.Immutable {
				a.PlanModifiers = []planmodifier.String{stringplanmodifier.RequiresReplace()}
			}
			attrs[name] = a
		case "bool":
			a := schema.BoolAttribute{Required: p.Required, Optional: !p.Required, Sensitive: p.Sensitive, Description: p.Description}
			if p.Immutable {
				a.PlanModifiers = []planmodifier.Bool{boolplanmodifier.RequiresReplace()}
			}
			attrs[name] = a
		case "int64":
			a := schema.Int64Attribute{Required: p.Required, Optional: !p.Required, Sensitive: p.Sensitive, Description: p.Description}
			if p.Immutable {
				a.PlanModifiers = []planmodifier.Int64{int64planmodifier.RequiresReplace()}
			}
			attrs[name] = a
		case "float64":
			a := schema.Float64Attribute{Required: p.Required, Optional: !p.Required, Sensitive: p.Sensitive, Description: p.Description}
			if p.Immutable {
				a.PlanModifiers = []planmodifier.Float64{float64planmodifier.RequiresReplace()}
			}
			attrs[name] = a
		}
	}
	return schema.Schema{Attributes: attrs}
}
func (r *infraResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	if req.ProviderData == nil {
		return
	}
	var ok bool
	r.client, ok = req.ProviderData.(*clientRuntime)
	if !ok {
		resp.Diagnostics.AddError("Unexpected provider client", "The provider returned an incompatible service client.")
	}
}

type infraReader interface {
	GetAttribute(context.Context, path.Path, any) diag.Diagnostics
}

func infraValue(ctx context.Context, source infraReader, name string, kind string, diagnostics *diag.Diagnostics) attr.Value {
	switch kind {
	case "string":
		var v types.String
		diagnostics.Append(source.GetAttribute(ctx, path.Root(name), &v)...)
		return v
	case "bool":
		var v types.Bool
		diagnostics.Append(source.GetAttribute(ctx, path.Root(name), &v)...)
		return v
	case "int64":
		var v types.Int64
		diagnostics.Append(source.GetAttribute(ctx, path.Root(name), &v)...)
		return v
	default:
		var v types.Float64
		diagnostics.Append(source.GetAttribute(ctx, path.Root(name), &v)...)
		return v
	}
}
func infraNative(value attr.Value) any {
	switch v := value.(type) {
	case types.String:
		return v.ValueString()
	case types.Bool:
		return v.ValueBool()
	case types.Int64:
		return v.ValueInt64()
	case types.Float64:
		return v.ValueFloat64()
	}
	return nil
}
func (r *infraResource) ValidateConfig(ctx context.Context, req resource.ValidateConfigRequest, resp *resource.ValidateConfigResponse) {
	for _, name := range infraKeys(r.desc.Properties) {
		p := r.desc.Properties[name]
		v := infraValue(ctx, req.Config, name, p.Kind, &resp.Diagnostics)
		if v.IsNull() || v.IsUnknown() {
			continue
		}
		infraValidate(name, p, infraNative(v), &resp.Diagnostics)
	}
}
func infraValidate(name string, p infraProperty, value any, diagnostics *diag.Diagnostics) {
	valid := true
	if text, ok := value.(string); ok {
		size := uint64(utf8.RuneCountInString(text))
		valid = (p.MinLength == nil || size >= *p.MinLength) && (p.MaxLength == nil || size <= *p.MaxLength)
		if len(p.Enum) > 0 {
			found := false
			for _, candidate := range p.Enum {
				if candidate == text {
					found = true
				}
			}
			valid = valid && found
		}
	}
	var number *big.Rat
	switch v := value.(type) {
	case int64:
		number = new(big.Rat).SetInt64(v)
	case float64:
		number, _ = new(big.Rat).SetString(strconv.FormatFloat(v, 'g', -1, 64))
		if number == nil {
			valid = false
		}
	}
	if number != nil {
		for _, bound := range []struct {
			value   *json.Number
			minimum bool
		}{{p.Min, true}, {p.Max, false}} {
			if bound.value == nil {
				continue
			}
			rational, ok := new(big.Rat).SetString(bound.value.String())
			if !ok {
				valid = false
				continue
			}
			comparison := number.Cmp(rational)
			valid = valid && (bound.minimum && comparison >= 0 || !bound.minimum && comparison <= 0)
		}
	}
	if !valid {
		diagnostics.AddAttributeError(path.Root(name), "Invalid resource property", "The value does not satisfy the model's constraints.")
	}
}
func infraKeys[T any](values map[string]T) []string {
	result := make([]string, 0, len(values))
	for k := range values {
		result = append(result, k)
	}
	sort.Strings(result)
	return result
}
func infraSet(root map[string]any, parts []string, value any) {
	for _, part := range parts[:len(parts)-1] {
		child, ok := root[part].(map[string]any)
		if !ok {
			child = map[string]any{}
			root[part] = child
		}
		root = child
	}
	root[parts[len(parts)-1]] = value
}
func infraGet(root map[string]any, parts []string) any {
	var value any = root
	for _, part := range parts {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		value = object[part]
	}
	return value
}
func (r *infraResource) input(ctx context.Context, stage string, source infraReader, diagnostics *diag.Diagnostics) map[string]any {
	result := map[string]any{}
	for _, name := range infraKeys(r.desc.Operations[stage].Input) {
		p := r.desc.Properties[name]
		if name == "id" {
			p = infraProperty{Kind: "string", Required: true}
		}
		value := infraValue(ctx, source, name, p.Kind, diagnostics)
		if value.IsUnknown() {
			diagnostics.AddAttributeError(path.Root(name), "Unknown resource property", "The value must be known before calling the service.")
			continue
		}
		if value.IsNull() {
			if p.Required {
				diagnostics.AddAttributeError(path.Root(name), "Missing resource property", "A required resource property is null.")
			}
			continue
		}
		native := infraNative(value)
		infraValidate(name, p, native, diagnostics)
		infraSet(result, r.desc.Operations[stage].Input[name], native)
	}
	return result
}
func (r *infraResource) call(ctx context.Context, stage string, input map[string]any) (map[string]any, error) {
	if r.client == nil {
		return nil, errors.New("provider is not configured")
	}
	return r.client.callValues(ctx, r.desc.Operations[stage].Name, input)
}
func (r *infraResource) missing(err error) bool {
	var serviceErr *ServiceError
	if !errors.As(err, &serviceErr) {
		return false
	}
	// The SDK also supports status-only error decoding. Dropping managed state
	// needs stronger evidence: an explicitly named absence response from the API.
	if explicitErrorName(serviceErr) != serviceErr.Name {
		return false
	}
	for _, name := range r.desc.NotFoundErrors {
		if name == serviceErr.Name {
			return true
		}
	}
	return false
}
// explicitErrorName is the error name the response itself carried, normalised
// the way the client normalises it into ServiceError.Name. Where the name lives
// is protocol-specific: a body __type for the plain JSON protocol, and for the
// AWS JSON protocols also a body code or the X-Amzn-Errortype header (restJson1
// bodies carry no __type; awsJson prefixes it with the namespace).
func explicitErrorName(e *ServiceError) string {
	var envelope struct {
		Type string `json:"__type"`
		Code string `json:"code"`
	}
	// An unparseable body simply carries no name.
	_ = json.Unmarshal(e.Metadata.Body, &envelope)
	name := envelope.Type
	if header := e.Metadata.Headers.Get("X-Crucible-Error"); header != "" {
		name = header
	}
	switch wireSchema.Protocol {
	case "awsRestJson1", "awsJson1_0", "awsJson1_1":
		if envelope.Code != "" {
			name = envelope.Code
		}
		if header := e.Metadata.Headers.Get("X-Amzn-Errortype"); header != "" {
			name = header
		}
		name = strings.SplitN(name, ":", 2)[0]
		if index := strings.LastIndex(name, "#"); index >= 0 {
			name = name[index+1:]
		}
	}
	return name
}
func infraError(stage string, err error, diagnostics *diag.Diagnostics) {
	// Server messages and transport errors can contain credentials or sensitive
	// resource values. Expose only modelled error identity and HTTP status.
	message := "The service request failed. Resource state was preserved; inspect service logs before retrying a mutation."
	var serviceErr *ServiceError
	if errors.As(err, &serviceErr) {
		name := "UnmodelledError"
		for _, shape := range wireSchema.Shapes {
			if shape.Error != nil && shape.Name == serviceErr.Name {
				name = shape.Name
				break
			}
		}
		message = fmt.Sprintf("Service error %s (HTTP %d). Inspect service logs before retrying a mutation.", name, serviceErr.Metadata.Status)
	}
	diagnostics.AddError(stage+" failed", message)
}
// importedKey marks state that holds only an imported id. Optional properties
// are unmanaged until configured, but an import has no configuration to defer
// to, so the first Read adopts the service's values; otherwise setting one later
// would diff null against the real value and replace the resource.
const importedKey = "imported"

func (r *infraResource) refresh(ctx context.Context, state *tfsdk.State, diagnostics *diag.Diagnostics, allowAbsent, imported bool) {
	input := r.input(ctx, "Read", *state, diagnostics)
	if diagnostics.HasError() {
		return
	}
	output, err := r.call(ctx, "Read", input)
	if err != nil {
		if allowAbsent && r.missing(err) {
			state.RemoveResource(ctx)
		} else {
			infraError("Read", err, diagnostics)
		}
		return
	}
	id, ok := infraGet(output, r.desc.Operations["Read"].Output["id"]).(string)
	var prior types.String
	diagnostics.Append(state.GetAttribute(ctx, path.Root("id"), &prior)...)
	if !ok || id == "" || id != prior.ValueString() {
		diagnostics.AddError("Invalid resource identity", "Read returned an absent or different resource identifier; existing state was preserved.")
		return
	}
	values := map[string]attr.Value{}
	for _, name := range infraKeys(r.desc.Properties) {
		p := r.desc.Properties[name]
		// An omitted optional property is not managed. Keep it null even when
		// the service supplies a default, preserving Terraform's planned value.
		if !p.Required && !imported {
			prior := infraValue(ctx, *state, name, p.Kind, diagnostics)
			if prior.IsNull() {
				continue
			}
		}
		raw := infraGet(output, r.desc.Operations["Read"].Output[name])
		v, err := infraDecode(p.Kind, raw)
		if err != nil || p.Required && v.IsNull() {
			diagnostics.AddError("Invalid service response", "Read returned a missing or invalid property: "+name)
			return
		}
		values[name] = v
	}
	for _, name := range infraKeys(values) {
		diagnostics.Append(state.SetAttribute(ctx, path.Root(name), values[name])...)
	}
}
func infraDecode(kind string, value any) (attr.Value, error) {
	switch kind {
	case "string":
		if value == nil {
			return types.StringNull(), nil
		}
		if v, ok := value.(string); ok {
			return types.StringValue(v), nil
		}
	case "bool":
		if value == nil {
			return types.BoolNull(), nil
		}
		if v, ok := value.(bool); ok {
			return types.BoolValue(v), nil
		}
	case "int64":
		if value == nil {
			return types.Int64Null(), nil
		}
		if v, ok := value.(json.Number); ok {
			n, e := v.Int64()
			return types.Int64Value(n), e
		}
	case "float64":
		if value == nil {
			return types.Float64Null(), nil
		}
		if v, ok := value.(json.Number); ok {
			n, e := v.Float64()
			return types.Float64Value(n), e
		}
	}
	return types.StringNull(), errors.New("invalid scalar response")
}
func (r *infraResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	input := r.input(ctx, "Create", req.Plan, &resp.Diagnostics)
	if resp.Diagnostics.HasError() {
		return
	}
	output, err := r.call(ctx, "Create", input)
	if err != nil {
		infraError("Create", err, &resp.Diagnostics)
		return
	}
	id, ok := infraGet(output, r.desc.Operations["Create"].Output["id"]).(string)
	if !ok || id == "" {
		resp.Diagnostics.AddError("Create returned no identity", "The service may have created a resource, but returned no identifier. Reconcile the service before retrying.")
		return
	}
	// Preserve the new identity even if the subsequent read fails, so Terraform
	// can recover and destroy the object instead of leaking it.
	resp.State.Raw = req.Plan.Raw
	resp.Diagnostics.Append(resp.State.SetAttribute(ctx, path.Root("id"), types.StringValue(id))...)
	if !resp.Diagnostics.HasError() {
		r.refresh(ctx, &resp.State, &resp.Diagnostics, false, false)
	}
}
func (r *infraResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	resp.State = req.State
	imported := false
	if req.Private != nil {
		marker, diags := req.Private.GetKey(ctx, importedKey)
		resp.Diagnostics.Append(diags...)
		imported = len(marker) > 0
	}
	r.refresh(ctx, &resp.State, &resp.Diagnostics, true, imported)
	if imported && !resp.Diagnostics.HasError() && resp.Private != nil {
		// Consume the marker so later reads keep unset optional properties null.
		resp.Diagnostics.Append(resp.Private.SetKey(ctx, importedKey, nil)...)
	}
}
func (r *infraResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	resp.State = req.State
	if r.desc.Operations["Update"].Name == "" {
		resp.Diagnostics.AddError("Replacement required", "This resource has no update operation. Its properties must be replaced.")
		return
	}
	input := r.input(ctx, "Update", req.Plan, &resp.Diagnostics)
	if resp.Diagnostics.HasError() {
		return
	}
	_, err := r.call(ctx, "Update", input)
	if err != nil {
		infraError("Update", err, &resp.Diagnostics)
		return
	}
	resp.State.Raw = req.Plan.Raw
	r.refresh(ctx, &resp.State, &resp.Diagnostics, false, false)
}
func (r *infraResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	input := r.input(ctx, "Delete", req.State, &resp.Diagnostics)
	if resp.Diagnostics.HasError() {
		return
	}
	_, err := r.call(ctx, "Delete", input)
	if err != nil && !r.missing(err) {
		infraError("Delete", err, &resp.Diagnostics)
		return
	}
	resp.State.RemoveResource(ctx)
}
func (*infraResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
	if resp.Private != nil {
		resp.Diagnostics.Append(resp.Private.SetKey(ctx, importedKey, []byte(`true`))...)
	}
}
