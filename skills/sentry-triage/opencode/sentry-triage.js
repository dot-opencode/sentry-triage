import { readFileSync, realpathSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

let moduleDirectory = dirname(fileURLToPath(import.meta.url))
try {
  moduleDirectory = dirname(realpathSync(fileURLToPath(import.meta.url)))
} catch {
  // Fall back to the unresolved path.
}
const skillDirectory = resolve(moduleDirectory, "..")
const bundledCli = resolve(skillDirectory, "bin", "triage")
const bundledSkillPath = resolve(skillDirectory, "SKILL.md")

function parseSkill(raw) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(raw)
  let description = ""
  if (match) {
    for (const line of match[1].split(/\r?\n/)) {
      const kv = /^description:\s*(.+)$/.exec(line)
      if (kv) description = kv[1].trim().replace(/^['"]|['"]$/g, "")
    }
  }
  return { description, body: (match ? raw.slice(match[0].length) : raw).trim() }
}

// A plain V2 plugin object ({ id, setup }); no imports from @opencode packages.
// It only registers the bundled skill and tells the agent where the CLI lives.
export default {
  id: "sentry-triage",
  async setup(ctx, deps = {}) {
    const cli = deps.cliPath ?? bundledCli
    let registration
    try {
      if (typeof ctx?.skill?.transform === "function") {
        const mdPath = deps.skillPath ?? bundledSkillPath
        const raw = readFileSync(mdPath, "utf8").replace(/^\uFEFF/, "")
        const parsed = parseSkill(raw)
        const content = `The \`triage\` CLI for this skill is \`${cli}\` (use this path if \`triage\` is not on PATH).\n\n${parsed.body}`
        registration = await ctx.skill.transform((editor) => {
          try {
            if (typeof editor?.add !== "function") return
            if (typeof editor.get === "function" && editor.get("sentry-triage")) return
            editor.add({ id: "sentry-triage", name: "sentry-triage", description: parsed.description, path: mdPath, content })
          } catch {
            // Registration must not throw.
          }
        })
      }
    } catch {
      // Never throw from setup.
    }

    return () => {
      try {
        registration?.dispose?.()
      } catch {
        // Cleanup must not throw.
      }
    }
  },
}
