#include "leyline_client/client.h"

static const leyline_string schema_chunks[] = {
    {"{\"service\":\"example.leyline#Leyline\",\"version\":\"2026-10-01\",\"shapes\":{\"crucible.errors#InternalServerException\":{\"kind\":\"structure\",\"members\":{\"message\":{\"target\":\"smithy.api#String\",\"attribute\":\"message\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"retryAfterSeconds\":{\"target\":\"smithy.api#Integer\",\"attribute\":\"retry_after_seconds\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"}},\"error\":{\"status\":500,\"fault\":\"server\",\"retryable\":true,\"throttling\":false,\"heade", 512},
    {"rs\":[[\"retryAfterSeconds\",\"Retry-After\"]]},\"name\":\"InternalServerException\"},\"crucible.errors#NotFoundException\":{\"kind\":\"structure\",\"members\":{\"message\":{\"target\":\"smithy.api#String\",\"attribute\":\"message\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"resourceType\":{\"target\":\"smithy.api#String\",\"attribute\":\"resource_type\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"error\":{\"status\":404,\"fault\":\"client\",\"retryable\":false,\"throttling\":false,\"headers\":[]},\"n", 512},
    {"ame\":\"NotFoundException\"},\"crucible.errors#ValidationException\":{\"kind\":\"structure\",\"members\":{\"message\":{\"target\":\"smithy.api#String\",\"attribute\":\"message\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"reason\":{\"target\":\"crucible.errors#ValidationExceptionReason\",\"attribute\":\"reason\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"fieldList\":{\"target\":\"crucible.errors#ValidationExceptionFieldList\",\"attribute\":\"field_list\",\"required\":false,\"default\":null,\"opti", 512},
    {"onal\":true,\"format\":\"date-time\"}},\"error\":{\"status\":400,\"fault\":\"client\",\"retryable\":false,\"throttling\":false,\"headers\":[]},\"name\":\"ValidationException\"},\"crucible.errors#ValidationExceptionField\":{\"kind\":\"structure\",\"members\":{\"name\":{\"target\":\"smithy.api#String\",\"attribute\":\"name\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"message\":{\"target\":\"smithy.api#String\",\"attribute\":\"message\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"ValidationExcepti", 512},
    {"onField\"},\"crucible.errors#ValidationExceptionFieldList\":{\"kind\":\"list\",\"member\":{\"target\":\"crucible.errors#ValidationExceptionField\",\"attribute\":\"member\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"},\"name\":\"ValidationExceptionFieldList\"},\"crucible.errors#ValidationExceptionReason\":{\"kind\":\"enum\",\"values\":[\"unknownOperation\",\"cannotParse\",\"fieldValidationFailed\",\"other\"],\"name\":\"ValidationExceptionReason\"},\"example.leyline#ArchiveGraphInput\":{\"kind\":\"structure\",\"members\":{\"organizat", 512},
    {"ionId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"graphId\":{\"target\":\"example.leyline#GraphId\",\"attribute\":\"graph_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"ArchiveGraphInput\"},\"example.leyline#ArchiveGraphOutput\":{\"kind\":\"structure\",\"members\":{\"archived\":{\"target\":\"smithy.api#Boolean\",\"attribute\":\"archived\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"dat", 512},
    {"e-time\"}},\"name\":\"ArchiveGraphOutput\"},\"example.leyline#CreateGraphInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"name\":{\"target\":\"smithy.api#String\",\"attribute\":\"name\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"CreateGraphInput\"},\"example.leyline#CreateGraphOutput\":{\"kind\":\"structure\",\"members\":{\"created\":{\"target\":\"examp", 512},
    {"le.leyline#GraphId\",\"attribute\":\"created\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"CreateGraphOutput\"},\"example.leyline#ExecuteQueryInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"graphId\":{\"target\":\"example.leyline#GraphId\",\"attribute\":\"graph_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"quer", 512},
    {"y\":{\"target\":\"smithy.api#String\",\"attribute\":\"query\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"target\":{\"target\":\"example.leyline#QueryTarget\",\"attribute\":\"target\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"}},\"name\":\"ExecuteQueryInput\"},\"example.leyline#ExecuteQueryOutput\":{\"kind\":\"structure\",\"members\":{\"rows\":{\"target\":\"example.leyline#Names\",\"attribute\":\"rows\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"ExecuteQueryO", 512},
    {"utput\"},\"example.leyline#GetGraphInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"graph\":{\"target\":\"example.leyline#GraphId\",\"attribute\":\"graph\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"GetGraphInput\"},\"example.leyline#GetGraphOutput\":{\"kind\":\"structure\",\"members\":{\"name\":{\"target\":\"smithy.api#String\",\"attribute\":\"name\",\"r", 512},
    {"equired\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"GetGraphOutput\"},\"example.leyline#GetOrganizationInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"GetOrganizationInput\"},\"example.leyline#GetOrganizationOutput\":{\"kind\":\"structure\",\"members\":{\"name\":{\"target\":\"smithy.api#String\",\"attribute\":\"name\",\"required\":true,\"default\":nul", 512},
    {"l,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"GetOrganizationOutput\"},\"example.leyline#GetUserOutput\":{\"kind\":\"structure\",\"members\":{\"subject\":{\"target\":\"smithy.api#String\",\"attribute\":\"subject\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"GetUserOutput\"},\"example.leyline#GraphId\":{\"kind\":\"string\",\"format\":\"date-time\",\"name\":\"GraphId\"},\"example.leyline#HealthOutput\":{\"kind\":\"structure\",\"members\":{\"status\":{\"target\":\"smithy.api#String\",\"attribute\":\"status\",\"required\":tru", 512},
    {"e,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"HealthOutput\"},\"example.leyline#HistoryId\":{\"kind\":\"string\",\"format\":\"date-time\",\"name\":\"HistoryId\"},\"example.leyline#HistoryRef\":{\"kind\":\"structure\",\"members\":{\"historyId\":{\"target\":\"example.leyline#HistoryId\",\"attribute\":\"history_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"HistoryRef\"},\"example.leyline#ListGraphsInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#Organizati", 512},
    {"onId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"ListGraphsInput\"},\"example.leyline#ListGraphsOutput\":{\"kind\":\"structure\",\"members\":{\"names\":{\"target\":\"example.leyline#Names\",\"attribute\":\"names\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"ListGraphsOutput\"},\"example.leyline#Names\":{\"kind\":\"list\",\"member\":{\"target\":\"smithy.api#String\",\"attribute\":\"member\",\"required\":false,\"default\":null,\"optional\":true,\"forma", 512},
    {"t\":\"date-time\"},\"name\":\"Names\"},\"example.leyline#OrganizationId\":{\"kind\":\"string\",\"format\":\"date-time\",\"name\":\"OrganizationId\"},\"example.leyline#QueryTarget\":{\"kind\":\"union\",\"members\":{\"current\":{\"target\":\"smithy.api#Unit\",\"attribute\":\"current\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"},\"history\":{\"target\":\"example.leyline#HistoryRef\",\"attribute\":\"history\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"}},\"name\":\"QueryTarget\"},\"example.leyline#RestoreBackupInp", 512},
    {"ut\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"graphId\":{\"target\":\"example.leyline#GraphId\",\"attribute\":\"graph_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"historyId\":{\"target\":\"example.leyline#HistoryId\",\"attribute\":\"history_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"RestoreBackupInput\"},\"ex", 512},
    {"ample.leyline#RestoreBackupOutput\":{\"kind\":\"structure\",\"members\":{\"restored\":{\"target\":\"smithy.api#Boolean\",\"attribute\":\"restored\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"RestoreBackupOutput\"},\"example.leyline#RewriteGraphInput\":{\"kind\":\"structure\",\"members\":{\"organizationId\":{\"target\":\"example.leyline#OrganizationId\",\"attribute\":\"organization_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"graphId\":{\"target\":\"example.leyline#GraphId\",\"attrib", 512},
    {"ute\":\"graph_id\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"},\"mode\":{\"target\":\"smithy.api#String\",\"attribute\":\"mode\",\"required\":false,\"default\":null,\"optional\":true,\"format\":\"date-time\"}},\"name\":\"RewriteGraphInput\"},\"example.leyline#RewriteGraphOutput\":{\"kind\":\"structure\",\"members\":{\"rewritten\":{\"target\":\"smithy.api#Boolean\",\"attribute\":\"rewritten\",\"required\":true,\"default\":null,\"optional\":false,\"format\":\"date-time\"}},\"name\":\"RewriteGraphOutput\"},\"smithy.api#Boolean\":{\"kind\":\"boolea", 512},
    {"n\",\"format\":\"date-time\",\"name\":\"Boolean\"},\"smithy.api#Integer\":{\"kind\":\"integer\",\"format\":\"date-time\",\"name\":\"Integer\"},\"smithy.api#String\":{\"kind\":\"string\",\"format\":\"date-time\",\"name\":\"String\"},\"smithy.api#Unit\":{\"kind\":\"unit\",\"name\":\"Unit\"}},\"operations\":{\"ArchiveGraph\":{\"method\":\"POST\",\"uri\":\"/orgs/{organizationId}/graphs/{graphId}/archive\",\"input\":\"example.leyline#ArchiveGraphInput\",\"output\":\"example.leyline#ArchiveGraphOutput\",\"request\":{\"labels\":[\"organizationId\",\"graphId\"],\"query\":[],\"headers\":[],\"bo", 512},
    {"dy\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"archived\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":false,\"token\":null,\"pagination\":null,\"bearer\":true},\"CreateGraph\":{\"method\":\"POST\",\"uri\":\"/orgs/{organizationId}/graphs\",\"input\":\"example.leyline#CreateGraphInput\",\"output\":\"example.leyline#CreateGraphOutput\",\"request\":{\"labels\":[\"organizationId\"],\"query\":[],\"headers\":[]", 512},
    {",\"body\":[\"name\"],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"created\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":false,\"token\":null,\"pagination\":null,\"bearer\":true},\"ExecuteQuery\":{\"method\":\"POST\",\"uri\":\"/orgs/{organizationId}/graphs/{graphId}/query\",\"input\":\"example.leyline#ExecuteQueryInput\",\"output\":\"example.leyline#ExecuteQueryOutput\",\"request\":{\"labels\":[\"organization", 512},
    {"Id\",\"graphId\"],\"query\":[],\"headers\":[],\"body\":[\"query\",\"target\"],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"rows\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":false,\"token\":null,\"pagination\":null,\"bearer\":true},\"GetGraph\":{\"method\":\"GET\",\"uri\":\"/orgs/{organizationId}/graphs/{graph}\",\"input\":\"example.leyline#GetGraphInput\",\"output\":\"example.leyline#GetGraphOutput\",\"request\":", 512},
    {"{\"labels\":[\"organizationId\",\"graph\"],\"query\":[],\"headers\":[],\"body\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"name\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":true,\"token\":null,\"pagination\":null,\"bearer\":true},\"GetOrganization\":{\"method\":\"GET\",\"uri\":\"/orgs/{organizationId}\",\"input\":\"example.leyline#GetOrganizationInput\",\"output\":\"example.leyline#GetOrganizationOutput\"", 512},
    {",\"request\":{\"labels\":[\"organizationId\"],\"query\":[],\"headers\":[],\"body\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"name\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":true,\"token\":null,\"pagination\":null,\"bearer\":true},\"GetUser\":{\"method\":\"GET\",\"uri\":\"/user\",\"input\":null,\"output\":\"example.leyline#GetUserOutput\",\"request\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[],\"paylo", 512},
    {"ad\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"subject\"],\"payload\":null},\"errors\":[\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":true,\"token\":null,\"pagination\":null,\"bearer\":true},\"Health\":{\"method\":\"GET\",\"uri\":\"/health\",\"input\":null,\"output\":\"example.leyline#HealthOutput\",\"request\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"status\"],\"payload\":null},\"errors\":[\"crucible.e", 512},
    {"rrors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":true,\"token\":null,\"pagination\":null,\"bearer\":false},\"ListGraphs\":{\"method\":\"GET\",\"uri\":\"/orgs/{organizationId}/graphs\",\"input\":\"example.leyline#ListGraphsInput\",\"output\":\"example.leyline#ListGraphsOutput\",\"request\":{\"labels\":[\"organizationId\"],\"query\":[],\"headers\":[],\"body\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"names\"],\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.erro", 512},
    {"rs#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":true,\"token\":null,\"pagination\":null,\"bearer\":true},\"RestoreBackup\":{\"method\":\"POST\",\"uri\":\"/orgs/{organizationId}/graphs/{graphId}/histories/{historyId}/restore\",\"input\":\"example.leyline#RestoreBackupInput\",\"output\":\"example.leyline#RestoreBackupOutput\",\"request\":{\"labels\":[\"organizationId\",\"graphId\",\"historyId\"],\"query\":[],\"headers\":[],\"body\":[],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"restored\"],\"payl", 512},
    {"oad\":null},\"errors\":[\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":false,\"token\":null,\"pagination\":null,\"bearer\":true},\"RewriteGraph\":{\"method\":\"POST\",\"uri\":\"/orgs/{organizationId}/graphs/{graphId}/rewrite\",\"input\":\"example.leyline#RewriteGraphInput\",\"output\":\"example.leyline#RewriteGraphOutput\",\"request\":{\"labels\":[\"organizationId\",\"graphId\"],\"query\":[],\"headers\":[],\"body\":[\"mode\"],\"payload\":null},\"response\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[\"rewritten\"]", 512},
    {",\"payload\":null},\"errors\":[\"crucible.errors#NotFoundException\",\"crucible.errors#ValidationException\",\"crucible.errors#InternalServerException\"],\"safe\":false,\"token\":null,\"pagination\":null,\"bearer\":true}},\"bearer\":true,\"apiVersion\":\"2026-10-01\",\"permissions\":[{\"resourceType\":\"graph\",\"identifiers\":[\"organizationId\",\"graphId\"],\"permissions\":[{\"symbol\":\"audit\",\"value\":\"graph:audit\",\"description\":\"\",\"tier\":\"permissionOnly\",\"grantable\":true,\"deprecated\":false},{\"symbol\":\"read\",\"value\":\"graph:read\",\"description\":\"", 512},
    {"Read a graph and its catalog.\",\"tier\":\"public\",\"grantable\":true,\"deprecated\":false},{\"symbol\":\"view\",\"value\":\"graph:view\",\"description\":\"\",\"tier\":\"public\",\"grantable\":true,\"deprecated\":true,\"aliasOf\":\"graph:read\"},{\"symbol\":\"write\",\"value\":\"graph:write\",\"description\":\"Change a graph: */ ends no comment and [bare] links nowhere.\\n@param is not a tag.\",\"tier\":\"public\",\"grantable\":true,\"deprecated\":false}]},{\"resourceType\":\"history\",\"identifiers\":[\"organizationId\",\"graphId\",\"historyId\"],\"permissions\":[{\"symbol", 512},
    {"\":\"read\",\"value\":\"history:read\",\"description\":\"\",\"tier\":\"public\",\"grantable\":true,\"deprecated\":false},{\"symbol\":\"restore\",\"value\":\"history:restore\",\"description\":\"\",\"tier\":\"permissionOnly\",\"grantable\":true,\"deprecated\":false}]},{\"resourceType\":\"org\",\"identifiers\":[\"organizationId\"],\"permissions\":[{\"symbol\":\"admin\",\"value\":\"organization:admin\",\"description\":\"Everything in the organization.\",\"tier\":\"notGrantable\",\"grantable\":false,\"deprecated\":false}]}]}", 456},
};
leyline_status leyline_client_new(const leyline_client_config *config, leyline_client **output, leyline_error *error) {
    const leyline_string endpoint = {"", 0};
    return leyline_runtime_new(config, schema_chunks, sizeof(schema_chunks) / sizeof(schema_chunks[0]), endpoint, output, error);
}
leyline_status leyline_operations_archive_graph(leyline_client *client, const leyline_models_archive_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_archive_graph_result *result, leyline_error *error) {
    const leyline_string operation = {"ArchiveGraph", 12};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_archive_graph_output *)result->base.output;
    return status;
}
void leyline_operations_archive_graph_result_free(leyline_operations_archive_graph_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_create_graph(leyline_client *client, const leyline_models_create_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_create_graph_result *result, leyline_error *error) {
    const leyline_string operation = {"CreateGraph", 11};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_create_graph_output *)result->base.output;
    return status;
}
void leyline_operations_create_graph_result_free(leyline_operations_create_graph_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_execute_query(leyline_client *client, const leyline_models_execute_query_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_execute_query_result *result, leyline_error *error) {
    const leyline_string operation = {"ExecuteQuery", 12};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_execute_query_output *)result->base.output;
    return status;
}
void leyline_operations_execute_query_result_free(leyline_operations_execute_query_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_get_graph(leyline_client *client, const leyline_models_get_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_graph_result *result, leyline_error *error) {
    const leyline_string operation = {"GetGraph", 8};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_get_graph_output *)result->base.output;
    return status;
}
void leyline_operations_get_graph_result_free(leyline_operations_get_graph_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_get_organization(leyline_client *client, const leyline_models_get_organization_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_organization_result *result, leyline_error *error) {
    const leyline_string operation = {"GetOrganization", 15};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_get_organization_output *)result->base.output;
    return status;
}
void leyline_operations_get_organization_result_free(leyline_operations_get_organization_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_get_user(leyline_client *client, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_user_result *result, leyline_error *error) {
    const leyline_string operation = {"GetUser", 7};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, NULL, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, NULL, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_get_user_output *)result->base.output;
    return status;
}
void leyline_operations_get_user_result_free(leyline_operations_get_user_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_health(leyline_client *client, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_health_result *result, leyline_error *error) {
    const leyline_string operation = {"Health", 6};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, NULL, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, NULL, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_health_output *)result->base.output;
    return status;
}
void leyline_operations_health_result_free(leyline_operations_health_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_list_graphs(leyline_client *client, const leyline_models_list_graphs_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_list_graphs_result *result, leyline_error *error) {
    const leyline_string operation = {"ListGraphs", 10};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_list_graphs_output *)result->base.output;
    return status;
}
void leyline_operations_list_graphs_result_free(leyline_operations_list_graphs_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_restore_backup(leyline_client *client, const leyline_models_restore_backup_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_restore_backup_result *result, leyline_error *error) {
    const leyline_string operation = {"RestoreBackup", 13};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_restore_backup_output *)result->base.output;
    return status;
}
void leyline_operations_restore_backup_result_free(leyline_operations_restore_backup_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
leyline_status leyline_operations_rewrite_graph(leyline_client *client, const leyline_models_rewrite_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_rewrite_graph_result *result, leyline_error *error) {
    const leyline_string operation = {"RewriteGraph", 12};
    leyline_status status;
    if (!result) return leyline_client_call(client, operation, input, cancelled, cancel_user, NULL, error);
    result->value = NULL;
    status = leyline_client_call(client, operation, input, cancelled, cancel_user, &result->base, error);
    if (status == leyline_ok) result->value = (const leyline_models_rewrite_graph_output *)result->base.output;
    return status;
}
void leyline_operations_rewrite_graph_result_free(leyline_operations_rewrite_graph_result *result) {
    if (result) { leyline_result_free(&result->base); result->value = NULL; }
}
const leyline_models_internal_server_exception *leyline_error_as_internal_server_exception(const leyline_result *result) {
    return result && result->error_shape == &leyline_shape_0 ? (const leyline_models_internal_server_exception *)result->modeled_error : NULL;
}
const leyline_models_not_found_exception *leyline_error_as_not_found_exception(const leyline_result *result) {
    return result && result->error_shape == &leyline_shape_1 ? (const leyline_models_not_found_exception *)result->modeled_error : NULL;
}
const leyline_models_validation_exception *leyline_error_as_validation_exception(const leyline_result *result) {
    return result && result->error_shape == &leyline_shape_2 ? (const leyline_models_validation_exception *)result->modeled_error : NULL;
}
