"use strict"

// Superfície mínima do SDK do host. O contexto é intencionalmente pequeno e
// capability-based: um plugin só recebe uma operação depois que a permissão
// declarada no manifest foi validada pelo registro.

const { SDK_API_VERSION, PLUGIN_PERMISSIONS, normalizeId, publicManifest } = require("./manifest")

class PluginPermissionError extends Error {
  constructor(permission) {
    super(`permissão não declarada: ${permission}`)
    this.name = "PluginPermissionError"
    this.code = "permission_denied"
    this.permission = permission
  }
}

function createPluginSdk({ registry, pluginId } = {}) {
  if (!registry || typeof registry.get !== "function" || typeof registry.hasPermission !== "function") {
    throw new TypeError("registry é obrigatório")
  }
  const id = normalizeId(pluginId)
  if (!id) throw new TypeError("pluginId inválido")

  function manifest() {
    const item = registry.get(id)
    return item?.manifest ? publicManifest(item.manifest) : null
  }

  function hasPermission(permission) {
    if (typeof permission !== "string") return false
    return registry.hasPermission(id, permission)
  }

  function assertPermission(permission) {
    if (!hasPermission(permission)) throw new PluginPermissionError(permission)
    return true
  }

  // `capability` is useful to host adapters: the implementation is not
  // callable until the manifest remains enabled and still grants its scope.
  function capability(permission, implementation) {
    if (typeof implementation !== "function") throw new TypeError("implementation é obrigatório")
    return (...args) => {
      assertPermission(permission)
      return implementation(...args)
    }
  }

  return Object.freeze({
    sdkVersion: SDK_API_VERSION,
    pluginId: id,
    permissions: Object.freeze([...PLUGIN_PERMISSIONS]),
    getManifest: manifest,
    hasPermission,
    assertPermission,
    capability,
  })
}

module.exports = { SDK_API_VERSION, PluginPermissionError, createPluginSdk }
