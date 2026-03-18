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

structure CreatePayloadSpecInput {
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
structure ListPayloadSpecsInput with [PaginationQuery] {}

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

structure GetPayloadSpecInput {
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

structure UpdatePayloadSpecInput {
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

structure DeletePayloadSpecInput {
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
