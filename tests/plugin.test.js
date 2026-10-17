import assert from "node:assert/strict"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"

import plugin from "../skills/sentry-triage/opencode/sentry-triage.js"

function tmp() {
  return mkdtempSync(join(tmpdir(), "sentry-triage-plugin-"))
}

function skillFile(root, description = "Test desc") {
  const path = join(root, "SKILL.md")
  writeFileSync(path, `---\nname: sentry-triage\ndescription: ${description}\n---\n\n# Body\ntext\n`)
  return path
}

function makeCtx({ existing = false } = {}) {
  const added = []
  const reg = { disposed: 0 }
  const ctx = {
    skill: {
      transform: async (fn) => {
        fn({ get: (id) => (existing && id === "sentry-triage" ? {} : undefined), add: (info) => void added.push(info) })
        return { dispose: () => void (reg.disposed += 1) }
      },
    },
  }
  return { ctx, added, reg }
}

test("plugin id", () => {
  assert.equal(plugin.id, "sentry-triage")
})

test("registers the skill with the absolute CLI path and disposes it", async () => {
  const root = tmp()
  try {
    const w = makeCtx()
    const path = skillFile(root)
    const cleanup = await plugin.setup(w.ctx, { skillPath: path, cliPath: "/abs/bin/triage" })
    assert.equal(w.added.length, 1)
    assert.deepEqual(w.added[0], {
      id: "sentry-triage",
      name: "sentry-triage",
      description: "Test desc",
      path,
      content:
        "The `triage` CLI for this skill is `/abs/bin/triage` (use this path if `triage` is not on PATH).\n\n# Body\ntext",
    })
    cleanup()
    assert.equal(w.reg.disposed, 1)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test("bundled SKILL.md registers with frontmatter stripped and an absolute cli path", async () => {
  const w = makeCtx()
  const cleanup = await plugin.setup(w.ctx)
  cleanup()
  assert.equal(w.added.length, 1)
  assert.ok(w.added[0].description.length > 10)
  assert.ok(!w.added[0].content.includes("name: sentry-triage"))
  const m = /^The `triage` CLI for this skill is `(\/[^`]+\/bin\/triage)`/.exec(w.added[0].content)
  assert.ok(m, w.added[0].content.slice(0, 200))
})

test("skips registration when sentry-triage already exists", async () => {
  const root = tmp()
  try {
    const w = makeCtx({ existing: true })
    const cleanup = await plugin.setup(w.ctx, { skillPath: skillFile(root) })
    assert.equal(w.added.length, 0)
    cleanup()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test("tolerates missing ctx.skill", async () => {
  const cleanup = await plugin.setup({})
  assert.equal(typeof cleanup, "function")
  cleanup()
})

test("does not throw when SKILL.md is missing or transform fails", async () => {
  const root = tmp()
  try {
    const w = makeCtx()
    const cleanup = await plugin.setup(w.ctx, { skillPath: join(root, "missing.md") })
    assert.equal(w.added.length, 0)
    cleanup()
    const cleanup2 = await plugin.setup(
      { skill: { transform: async () => { throw new Error("boom") } } },
      { skillPath: skillFile(root) },
    )
    cleanup2()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
