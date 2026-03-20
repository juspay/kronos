$version: "2"

namespace com.kronos

// ─── Payload Spec structures ─────────────────────────────────────

structure PayloadSpecResource {
    @required
    name: String

    @required
    schema: JsonObject

    @required
    created_at: DateTime

    @required
    updated_at: DateTime
}

list PayloadSpecList {
    member: PayloadSpecResource
}

structure PaginatedPayloadSpecs {
    @required
    data: PayloadSpecList

    cursor: String
}

// ─── Create ──────────────────────────────────────────────────────

@input
structure CreatePayloadSpecInput with [WorkspaceHeaders] {
    @required
    name: String

    @required
    schema: JsonObject
}

structure CreatePayloadSpecOutput {
    @required
    data: PayloadSpecResource
}

@http(method: "POST", uri: "/v1/payload-specs", code: 201)
@idempotent
operation CreatePayloadSpec {
    input: CreatePayloadSpecInput
    output: CreatePayloadSpecOutput
    errors: [InvalidRequestError, ConflictError]
}

// ─── List ────────────────────────────────────────────────────────

@input
structure ListPayloadSpecsInput with [WorkspaceHeaders, PaginationQuery] {}

structure ListPayloadSpecsOutput {
    @required
    data: PayloadSpecList

    cursor: String
}

@http(method: "GET", uri: "/v1/payload-specs", code: 200)
@readonly
operation ListPayloadSpecs {
    input: ListPayloadSpecsInput
    output: ListPayloadSpecsOutput
}

// ─── Get ─────────────────────────────────────────────────────────

@input
structure GetPayloadSpecInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    name: String
}

structure GetPayloadSpecOutput {
    @required
    data: PayloadSpecResource
}

@http(method: "GET", uri: "/v1/payload-specs/{name}", code: 200)
@readonly
operation GetPayloadSpec {
    input: GetPayloadSpecInput
    output: GetPayloadSpecOutput
    errors: [NotFoundError]
}

// ─── Update ──────────────────────────────────────────────────────

@input
structure UpdatePayloadSpecInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    name: String

    @required
    schema: JsonObject
}

structure UpdatePayloadSpecOutput {
    @required
    data: PayloadSpecResource
}

@http(method: "PUT", uri: "/v1/payload-specs/{name}", code: 200)
@idempotent
operation UpdatePayloadSpec {
    input: UpdatePayloadSpecInput
    output: UpdatePayloadSpecOutput
    errors: [NotFoundError, InvalidRequestError]
}

// ─── Delete ──────────────────────────────────────────────────────

@input
structure DeletePayloadSpecInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    name: String
}

@http(method: "DELETE", uri: "/v1/payload-specs/{name}", code: 204)
@idempotent
operation DeletePayloadSpec {
    input: DeletePayloadSpecInput
    errors: [NotFoundError]
}
