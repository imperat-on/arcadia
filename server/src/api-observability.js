"use strict"

// Cross-request observability shared by every HTTP endpoint. The legacy
// `error` field is deliberately kept: older clients use it as their machine
// readable code. New clients can use `code`, `message`, and `request_id`
// without parsing a localized string or correlating logs by timestamp.

const crypto = require("node:crypto")

const REQUEST_ID_HEADER = "X-Request-Id"
const REQUEST_ID_RE = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/

const DEFAULT_MESSAGES = {
  nao_autenticado: "Autenticacao necessaria",
  permissao_negada: "Permissao negada",
  rota_nao_encontrada: "Rota nao encontrada",
  json_invalido: "JSON invalido",
  payload_grande: "Payload excede o limite permitido",
  servico_indisponivel: "Servico indisponivel",
  erro_interno: "Erro interno do servidor",
}

function createRequestId(value) {
  const candidate = typeof value === "string" ? value.trim() : ""
  return REQUEST_ID_RE.test(candidate) ? candidate : crypto.randomUUID()
}

function requestContext(req, res, next) {
  const id = createRequestId(req.get?.(REQUEST_ID_HEADER) || req.headers?.["x-request-id"])
  req.requestId = id
  res.locals = res.locals || {}
  res.locals.requestId = id
  res.setHeader(REQUEST_ID_HEADER, id)
  next()
}

function errorMessage(code, fallback) {
  return fallback || DEFAULT_MESSAGES[code] || "A solicitacao nao pode ser concluida"
}

function decorateErrorBody(req, body) {
  if (!body || typeof body !== "object" || Array.isArray(body) || typeof body.error !== "string") {
    return body
  }
  const code = typeof body.code === "string" ? body.code : body.error
  return {
    ...body,
    code,
    message: typeof body.message === "string" ? body.message : errorMessage(code),
    request_id: typeof body.request_id === "string" ? body.request_id : req.requestId,
  }
}

// Existing route handlers send `{ error: "codigo" }` directly. Decorating
// res.json at the application boundary avoids changing every route (and keeps
// the old response contract intact). Only HTTP error responses are decorated;
// successful RPC payloads that happen to contain an `error` property stay
// byte-for-byte compatible.
function structuredErrors(req, res, next) {
  const json = res.json
  res.json = function jsonWithErrorDetails(body) {
    const decorated = res.statusCode >= 400 ? decorateErrorBody(req, body) : body
    return json.call(this, decorated)
  }
  next()
}

function sendError(req, res, status, code, options = {}) {
  const body = {
    error: code,
    code,
    message: errorMessage(code, options.message),
    request_id: req.requestId || res.getHeader(REQUEST_ID_HEADER) || null,
  }
  if (options.details !== undefined) body.details = options.details
  return res.status(status).json(body)
}

function exceptionStatus(error) {
  if (error?.type === "entity.too.large") return 413
  const status = Number(error?.statusCode ?? error?.status)
  return Number.isInteger(status) && status >= 400 && status < 500 ? status : 500
}

function exceptionCode(error, status) {
  if (error?.type === "entity.parse.failed") return "json_invalido"
  if (error?.type === "entity.too.large" || status === 413) return "payload_grande"
  return status >= 500 ? "erro_interno" : "erro_requisicao"
}

function handleError(error, req, res, next) {
  if (res.headersSent) return next(error)
  const status = exceptionStatus(error)
  const code = exceptionCode(error, status)
  if (status >= 500) console.error("[erro]", { request_id: req.requestId, error })
  return sendError(req, res, status, code)
}

module.exports = {
  REQUEST_ID_HEADER,
  REQUEST_ID_RE,
  createRequestId,
  requestContext,
  structuredErrors,
  sendError,
  exceptionStatus,
  exceptionCode,
  handleError,
}
