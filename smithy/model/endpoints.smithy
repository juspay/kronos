$version: "2"

namespace com.kronos

// ─── Endpoint structures ─────────────────────────────────────────

structure EndpointResource {
    @required
    name: String

    @required
    @jsonName("type")
    endpoint_type: EndpointTypeEnum

    payload_spec: String

    config: String

    @required
    spec: JsonObject

    retry_policy: RetryPolicy

    @required
    created_at: DateTime

    @required
    updated_at: DateTime
}

list EndpointList {
    member: EndpointResource
}

// ─── Create ──────────────────────────────────────────────────────

structure CreateEndpointInput {
    @required
    name: String

    @required
    @jsonName("type")
    endpoint_type: EndpointTypeEnum

    payload_spec: String

    config: String

    @required
    spec: JsonObject

    retry_policy: RetryPolicy
}

structure CreateEndpointOutput {
    @required
    data: EndpointResource
}

@http(method: "POST", uri: "/v1/endpoints", code: 201)
@idempotent
operation CreateEndpoint {
    input: CreateEndpointInput
    output: CreateEndpointOutput
    errors: [InvalidRequestError, ConflictError, UnprocessableEntityError]
}

// ─── List ────────────────────────────────────────────────────────

@input
structure ListEndpointsInput with [PaginationQuery] {}

structure ListEndpointsOutput {
    @required
    data: EndpointList

    cursor: String
}

@http(method: "GET", uri: "/v1/endpoints", code: 200)
@readonly
operation ListEndpoints {
    input: ListEndpointsInput
    output: ListEndpointsOutput
}

// ─── Get ─────────────────────────────────────────────────────────

structure GetEndpointInput {
    @required
    @httpLabel
    name: String
}

structure GetEndpointOutput {
    @required
    data: EndpointResource
}

@http(method: "GET", uri: "/v1/endpoints/{name}", code: 200)
@readonly
operation GetEndpoint {
    input: GetEndpointInput
    output: GetEndpointOutput
    errors: [NotFoundError]
}

// ─── Update ──────────────────────────────────────────────────────

structure UpdateEndpointInput {
    @required
    @httpLabel
    name: String

    spec: JsonObject

    config: String

    payload_spec: String

    retry_policy: RetryPolicy
}

structure UpdateEndpointOutput {
    @required
    data: EndpointResource
}

@http(method: "PUT", uri: "/v1/endpoints/{name}", code: 200)
@idempotent
operation UpdateEndpoint {
    input: UpdateEndpointInput
    output: UpdateEndpointOutput
    errors: [NotFoundError, InvalidRequestError, UnprocessableEntityError]
}

// ─── Delete ──────────────────────────────────────────────────────

structure DeleteEndpointInput {
    @required
    @httpLabel
    name: String
}

@http(method: "DELETE", uri: "/v1/endpoints/{name}", code: 204)
@idempotent
operation DeleteEndpoint {
    input: DeleteEndpointInput
    errors: [NotFoundError]
}
