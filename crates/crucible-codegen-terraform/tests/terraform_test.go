package provider

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/providerserver"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/tfsdk"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/hashicorp/terraform-plugin-go/tfprotov6"
	"github.com/hashicorp/terraform-plugin-go/tftypes"
)

type widgetModel struct {
	ID       types.String `tfsdk:"id"`
	Name     types.String `tfsdk:"name"`
	Enabled  types.Bool   `tfsdk:"enabled"`
	Capacity types.Int64  `tfsdk:"capacity"`
	Slug     types.String `tfsdk:"slug"`
}

func checkDiagnostics(t *testing.T, d diag.Diagnostics) {
	t.Helper()
	if d.HasError() {
		t.Fatalf("diagnostics: %v", d)
	}
}
func plan(t *testing.T, r *infraResource, name types.String, id types.String) tfsdk.Plan {
	t.Helper()
	p := tfsdk.Plan{Schema: r.schema()}
	checkDiagnostics(t, p.Set(context.Background(), widgetModel{ID: id, Name: name, Enabled: types.BoolValue(true), Capacity: types.Int64Value(3), Slug: types.StringNull()}))
	return p
}
func readModel(t *testing.T, state tfsdk.State) widgetModel {
	t.Helper()
	var model widgetModel
	checkDiagnostics(t, state.Get(context.Background(), &model))
	return model
}
func TestProtocolSixSchema(t *testing.T) {
	server := providerserver.NewProtocol6(New())()
	response, err := server.GetProviderSchema(context.Background(), &tfprotov6.GetProviderSchemaRequest{})
	if err != nil {
		t.Fatal(err)
	}
	for _, d := range response.Diagnostics {
		if d.Severity == tfprotov6.DiagnosticSeverityError {
			t.Fatal(d)
		}
	}
	if response.ResourceSchemas["inventory_widget"] == nil {
		t.Fatal("missing generated resource schema")
	}
}
func TestLifecycleOverHTTP(t *testing.T) {
	ctx := context.Background()
	stored := map[string]any{}
	requests := map[string]int{}
	mode := ""
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		requests[req.Method]++
		if req.Header.Get("Authorization") != "Bearer test-secret" {
			t.Errorf("credential callback not used")
		}
		w.Header().Set("Content-Type", "application/json")
		if mode == "failure" {
			w.WriteHeader(503)
			_, _ = w.Write([]byte(`{"__type":"Unavailable","message":"test-secret must not leak"}`))
			return
		}
		if req.Method == "POST" || req.Method == "PUT" {
			if err := json.NewDecoder(req.Body).Decode(&stored); err != nil {
				t.Error(err)
			}
			stored["id"] = "w-1"
		}
		if req.Method != "POST" && req.URL.Path != "/widgets/w-1" {
			t.Errorf("wrong resource identity path %s", req.URL.Path)
		}
		if len(stored) == 0 {
			w.WriteHeader(404)
			_, _ = w.Write([]byte(`{"__type":"NotFound","message":"not here"}`))
			return
		}
		if req.Method == "DELETE" {
			stored = map[string]any{}
			_, _ = w.Write([]byte(`{}`))
			return
		}
		output := stored
		if _, present := stored["slug"]; !present {
			output = map[string]any{}
			for name, value := range stored {
				output[name] = value
			}
			output["slug"] = "server-default"
		}
		if mode == "wrong-id" {
			output = map[string]any{"id": "someone-else"}
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"widget": output})
	}))
	defer server.Close()
	client, err := newClient(Config{Endpoint: server.URL, MaxAttempts: 1, Token: func(context.Context) (string, error) { return "test-secret", nil }}, wireSchema, false)
	if err != nil {
		t.Fatal(err)
	}
	r := &infraResource{desc: infraDescriptions[0], client: client}
	create := resource.CreateResponse{State: tfsdk.State{Schema: r.schema()}}
	r.Create(ctx, resource.CreateRequest{Plan: plan(t, r, types.StringValue("alpha"), types.StringUnknown())}, &create)
	checkDiagnostics(t, create.Diagnostics)
	state := create.State
	model := readModel(t, state)
	if model.ID.ValueString() != "w-1" || model.Name.ValueString() != "alpha" || !model.Slug.IsNull() {
		t.Fatal(model)
	}
	if _, present := stored["slug"]; present {
		t.Fatal("null optional property must be omitted")
	}
	update := resource.UpdateResponse{State: tfsdk.State{Schema: r.schema()}}
	r.Update(ctx, resource.UpdateRequest{State: state, Plan: plan(t, r, types.StringValue("beta"), model.ID)}, &update)
	checkDiagnostics(t, update.Diagnostics)
	state = update.State
	if readModel(t, state).Name.ValueString() != "beta" {
		t.Fatal("update did not refresh")
	}
	stored["name"] = "remote-drift"
	read := resource.ReadResponse{}
	r.Read(ctx, resource.ReadRequest{State: state}, &read)
	checkDiagnostics(t, read.Diagnostics)
	state = read.State
	if readModel(t, state).Name.ValueString() != "remote-drift" {
		t.Fatal("read hid drift")
	}
	mode = "wrong-id"
	read = resource.ReadResponse{}
	r.Read(ctx, resource.ReadRequest{State: state}, &read)
	if !read.Diagnostics.HasError() {
		t.Fatal("identity mismatch accepted")
	}
	if readModel(t, read.State).ID.ValueString() != "w-1" {
		t.Fatal("identity overwritten")
	}
	mode = ""
	imported := resource.ImportStateResponse{State: tfsdk.State{Schema: r.schema(), Raw: tftypes.NewValue(r.schema().Type().TerraformType(ctx), nil)}}
	r.ImportState(ctx, resource.ImportStateRequest{ID: "w-1"}, &imported)
	checkDiagnostics(t, imported.Diagnostics)
	read = resource.ReadResponse{}
	r.Read(ctx, resource.ReadRequest{State: imported.State}, &read)
	checkDiagnostics(t, read.Diagnostics)
	if readModel(t, read.State).Name.ValueString() != "remote-drift" {
		t.Fatal("import did not refresh")
	}
	deleted := resource.DeleteResponse{State: state}
	r.Delete(ctx, resource.DeleteRequest{State: state}, &deleted)
	checkDiagnostics(t, deleted.Diagnostics)
	if !deleted.State.Raw.IsNull() {
		t.Fatal("delete kept state")
	}
	deleted = resource.DeleteResponse{State: state}
	r.Delete(ctx, resource.DeleteRequest{State: state}, &deleted)
	checkDiagnostics(t, deleted.Diagnostics)
	read = resource.ReadResponse{}
	r.Read(ctx, resource.ReadRequest{State: state}, &read)
	checkDiagnostics(t, read.Diagnostics)
	if !read.State.Raw.IsNull() {
		t.Fatal("not found did not remove resource")
	}
	before := requests["POST"]
	mode = "failure"
	create = resource.CreateResponse{State: tfsdk.State{Schema: r.schema()}}
	r.Create(ctx, resource.CreateRequest{Plan: plan(t, r, types.StringValue("alpha"), types.StringUnknown())}, &create)
	if !create.Diagnostics.HasError() || requests["POST"] != before+1 {
		t.Fatal("unsafe create retry or missing error")
	}
	for _, d := range create.Diagnostics {
		if strings.Contains(d.Detail(), "test-secret") {
			t.Fatal("service error leaked secret")
		}
	}
	before = requests["POST"]
	create = resource.CreateResponse{State: tfsdk.State{Schema: r.schema()}}
	r.Create(ctx, resource.CreateRequest{Plan: plan(t, r, types.StringUnknown(), types.StringUnknown())}, &create)
	if !create.Diagnostics.HasError() || requests["POST"] != before {
		t.Fatal("unknown property reached HTTP")
	}
}

func TestConstraintPrecisionAndEnumValidation(t *testing.T) {
	max := json.Number("9007199254740992")
	var diagnostics diag.Diagnostics
	infraValidate("capacity", infraProperty{Max: &max}, int64(9007199254740993), &diagnostics)
	if !diagnostics.HasError() {
		t.Fatal("integer above 2^53 lost range precision")
	}
	diagnostics = nil
	infraValidate("name", infraProperty{Enum: []string{"ready", "pending"}}, "other", &diagnostics)
	if !diagnostics.HasError() {
		t.Fatal("invalid enum accepted")
	}
	r := &infraResource{desc: infraDescriptions[0]}
	if r.missing(&ServiceError{Name: "NotFound", Metadata: ResponseMetadata{Status: 404, Body: []byte(`{}`)}}) {
		t.Fatal("unnamed 404 dropped state")
	}
	if r.missing(&ServiceError{Name: "AccessDenied", Metadata: ResponseMetadata{Status: 404, Body: []byte(`{"__type":"AccessDenied"}`)}}) {
		t.Fatal("unrecognized error dropped state")
	}
}

func TestCreatePreservesIdentityWhenRefreshFails(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if r.Method == "POST" {
			_, _ = w.Write([]byte(`{"widget":{"id":"w-1"}}`))
			return
		}
		w.WriteHeader(503)
		_, _ = w.Write([]byte(`{"__type":"Unavailable"}`))
	}))
	defer server.Close()
	client, err := newClient(Config{Endpoint: server.URL, MaxAttempts: 1}, wireSchema, false)
	if err != nil {
		t.Fatal(err)
	}
	r := &infraResource{desc: infraDescriptions[0], client: client}
	response := resource.CreateResponse{State: tfsdk.State{Schema: r.schema()}}
	r.Create(context.Background(), resource.CreateRequest{Plan: plan(t, r, types.StringValue("alpha"), types.StringUnknown())}, &response)
	if !response.Diagnostics.HasError() {
		t.Fatal("failed refresh did not report error")
	}
	var id types.String
	checkDiagnostics(t, response.State.GetAttribute(context.Background(), path.Root("id"), &id))
	if id.ValueString() != "w-1" {
		t.Fatal("new identity lost")
	}
}
