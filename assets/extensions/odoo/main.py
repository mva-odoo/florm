"""The Odoo extension of Florm: its commands (extension.toml) on odoo_lib."""

import json
import os

import florm
import odoo_lib

STATE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "state.json")


def _state():
    try:
        with open(STATE, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def _save(state):
    with open(STATE, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=1)


def _last(ctx):
    """The last test command run in this project."""
    return _state().get("commands", {}).get(ctx.root or "")


def _remember(ctx, cmd):
    s = _state()
    s.setdefault("commands", {})[ctx.root or ""] = cmd
    _save(s)


def _need_file(ctx):
    if not ctx.file:
        florm.message("Ouvre d'abord un fichier d'un module Odoo", "warning")
        return False
    return True


def _run_tests(ctx, cmd):
    _remember(ctx, cmd)
    florm.status("Tests Odoo : " + cmd)
    florm.terminal(cmd)


@florm.command("test")
def test(ctx):
    if not _need_file(ctx):
        return
    tags = odoo_lib.tags_at(ctx.file, florm.text(), max(ctx.line - 1, 0))
    if not tags:
        florm.message("Ce fichier n'est pas dans un module Odoo (pas de __manifest__.py au-dessus)", "warning")
        return
    last = _last(ctx)
    cmd = odoo_lib.command(tags, last, odoo_lib.find_odoo_bin(ctx.root))
    if not last:
        # the first time: to complete (the database -d, -u, a docker compose exec in front)
        cmd = florm.input("Commande des tests Odoo (complète la base -d, un conteneur devant…)", cmd)
        if not cmd:
            return
    _run_tests(ctx, cmd)


@florm.command("test_edit")
def test_edit(ctx):
    tags = odoo_lib.tags_at(ctx.file, florm.text(), max(ctx.line - 1, 0)) if ctx.file else None
    last = _last(ctx)
    cmd = odoo_lib.command(tags, last, odoo_lib.find_odoo_bin(ctx.root)) if tags else (last or "")
    cmd = florm.input("Commande des tests Odoo", cmd)
    if cmd:
        _run_tests(ctx, cmd)


@florm.command("update")
def update(ctx):
    if not _need_file(ctx):
        return
    module = odoo_lib.module_of(ctx.file)
    if not module:
        florm.message("Ce fichier n'est pas dans un module Odoo", "warning")
        return
    cmd = florm.input("Mettre à jour le module", odoo_lib.update_command(module, _last(ctx), odoo_lib.find_odoo_bin(ctx.root)))
    if cmd:
        florm.status("Mise à jour : " + cmd)
        florm.terminal(cmd)


def _choose(hits, what):
    if not hits:
        florm.message("Rien trouvé pour « %s »" % what, "warning")
        return
    if len(hits) == 1:
        p, n, _, _ = hits[0]
        florm.open(p, n)
        return
    k = florm.pick([("%s  ·  %s" % (os.path.relpath(p), kind), "ligne %d : %s" % (n, text[:100])) for p, n, kind, text in hits], what)
    if k is not None:
        florm.open(hits[k][0], hits[k][1])


@florm.command("goto")
def goto(ctx):
    if not _need_file(ctx):
        return
    lines = florm.text().splitlines()
    line = lines[ctx.line - 1] if 0 < ctx.line <= len(lines) else ""
    word = ctx.selection.strip() or odoo_lib.word_at(line, max(ctx.col - 1, 0))
    if not word:
        florm.message("Place le curseur sur 'sale.order' ou 'module.xml_id'", "warning")
        return
    root = ctx.root or os.path.dirname(ctx.file)
    florm.status("Recherche de « %s »…" % word)
    hits = []
    # a model has a dot and is defined somewhere; else (or also) a record
    if "." in word:
        hits = odoo_lib.find_model(root, word)
    if not hits:
        hits = odoo_lib.find_xmlid(root, word, odoo_lib.module_of(ctx.file))
    _choose(hits, word)


@florm.command("records")
def records(ctx):
    if not _need_file(ctx):
        return
    items = odoo_lib.records_in(ctx.file, florm.text())
    if not items:
        florm.message("Pas de modèle ni de record dans ce fichier", "warning")
        return
    k = florm.pick([(label, "ligne %d" % n) for n, label in items], "Modèles et records")
    if k is not None:
        florm.open(ctx.file, items[k][0])


@florm.command("manifest")
def manifest(ctx):
    d = odoo_lib.module_dir(ctx.file) if ctx.file else None
    if not d:
        florm.message("Ce fichier n'est pas dans un module Odoo", "warning")
        return
    florm.open(os.path.join(d, "__manifest__.py"), 1)


florm.main()
