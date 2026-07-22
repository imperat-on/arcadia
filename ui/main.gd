extends Control
## Front-end estilo PS5: fileira de capas, hero art no fundo, navegação por
## controle e lançamento dos jogos de Steam/Heroic/Lutris.
##
## Lê ~/.local/share/arcadia/library.json (gerado por index.py).

const CARD_W := 230.0
const CARD_H := 345.0
const GAP := 34.0
const FOCUS_SCALE := 1.16
const DIM := Color(0.5, 0.52, 0.58, 1.0)
const BRIGHT := Color(1, 1, 1, 1)
const NAV_COOLDOWN := 0.16

var lib_path := OS.get_environment("HOME") + "/.local/share/arcadia/library.json"
var index_path := OS.get_environment("HOME") + "/.local/share/arcadia/index.py"

var games: Array = []
var cards: Array = []
var selected := 0
var nav_timer := 0.0
var _nav_tweens: Array = []
var _overlay_token := 0

# Nós construídos em _ready.
var bg_base: ColorRect
var bg_hero: TextureRect
var bg_shade: ColorRect
var logo_rect: TextureRect
var title_label: Label
var tag_label: Label
var hint_label: Label
var row: Control
var overlay: ColorRect
var overlay_label: Label


func _ready() -> void:
	_build_chrome()
	_load_library()
	_build_cards()
	if games.is_empty():
		title_label.text = "Nenhum jogo encontrado"
		tag_label.text = "Instale jogos ou rode o index.py"
	else:
		_select(0, true)
	_start_bg_animation()


# --------------------------------------------------------------------------- #
# Construção da UI
# --------------------------------------------------------------------------- #
func _build_chrome() -> void:
	bg_base = ColorRect.new()
	bg_base.color = Color(0.04, 0.05, 0.07)
	bg_base.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg_base)

	bg_hero = TextureRect.new()
	bg_hero.set_anchors_preset(Control.PRESET_FULL_RECT)
	bg_hero.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	bg_hero.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
	bg_hero.modulate = Color(0.55, 0.55, 0.6, 0.0)  # começa invisível; fade-in
	bg_hero.pivot_offset = Vector2(960, 540)
	add_child(bg_hero)

	# Gradiente escuro por cima do hero (legibilidade + vibe PS5).
	bg_shade = ColorRect.new()
	bg_shade.set_anchors_preset(Control.PRESET_FULL_RECT)
	bg_shade.material = _make_gradient_material()
	add_child(bg_shade)

	# Painel de informação (logo/título) no alto à esquerda.
	logo_rect = TextureRect.new()
	logo_rect.position = Vector2(120, 140)
	logo_rect.custom_minimum_size = Vector2(560, 240)
	logo_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	logo_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT
	add_child(logo_rect)

	title_label = Label.new()
	title_label.position = Vector2(122, 150)
	title_label.add_theme_font_size_override("font_size", 64)
	title_label.add_theme_color_override("font_shadow_color", Color(0, 0, 0, 0.6))
	title_label.add_theme_constant_override("shadow_offset_y", 3)
	add_child(title_label)

	tag_label = Label.new()
	tag_label.position = Vector2(124, 96)
	tag_label.add_theme_font_size_override("font_size", 24)
	tag_label.add_theme_color_override("font_color", Color(0.6, 0.78, 1.0))
	add_child(tag_label)

	# Fileira de capas.
	row = Control.new()
	row.position = Vector2(0, 560)
	add_child(row)

	hint_label = Label.new()
	hint_label.text = "◄ ►  navegar     A  jogar     início/select  atualizar"
	hint_label.add_theme_font_size_override("font_size", 22)
	hint_label.add_theme_color_override("font_color", Color(0.7, 0.72, 0.78))
	hint_label.position = Vector2(120, 1010)
	add_child(hint_label)

	# Overlay "Iniciando…".
	overlay = ColorRect.new()
	overlay.color = Color(0, 0, 0, 0.82)
	overlay.set_anchors_preset(Control.PRESET_FULL_RECT)
	overlay.visible = false
	add_child(overlay)
	overlay_label = Label.new()
	overlay_label.set_anchors_preset(Control.PRESET_CENTER)
	overlay_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	overlay_label.add_theme_font_size_override("font_size", 48)
	overlay_label.position = Vector2(760, 500)
	overlay.add_child(overlay_label)


func _make_gradient_material() -> ShaderMaterial:
	var sh := Shader.new()
	sh.code = """
shader_type canvas_item;
void fragment() {
	// Escurece nas bordas e embaixo, deixa o topo-esquerdo mais limpo.
	float v = smoothstep(0.35, 1.0, UV.y);
	float l = smoothstep(0.75, 0.15, UV.x) * 0.35;
	float a = clamp(v * 0.85 + l + 0.25, 0.0, 0.92);
	COLOR = vec4(0.02, 0.03, 0.05, a);
}
"""
	var mat := ShaderMaterial.new()
	mat.shader = sh
	return mat


func _build_cards() -> void:
	for c in cards:
		c.queue_free()
	cards.clear()
	for i in games.size():
		var g: Dictionary = games[i]
		var card := _make_card(g)
		card.position = Vector2(i * (CARD_W + GAP), 0)
		row.add_child(card)
		cards.append(card)


func _make_card(g: Dictionary) -> Control:
	var card := Control.new()
	card.custom_minimum_size = Vector2(CARD_W, CARD_H)
	card.size = Vector2(CARD_W, CARD_H)
	card.pivot_offset = Vector2(CARD_W / 2, CARD_H / 2)

	var tex := _load_texture(g.get("cover", ""))
	if tex != null:
		var tr := TextureRect.new()
		tr.texture = tex
		tr.size = Vector2(CARD_W, CARD_H)
		tr.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
		tr.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
		card.add_child(tr)
	else:
		# Fallback: card colorido com o título.
		var bg := ColorRect.new()
		bg.color = Color(0.11, 0.13, 0.18)
		bg.size = Vector2(CARD_W, CARD_H)
		card.add_child(bg)
		var lbl := Label.new()
		lbl.text = str(g.get("title", "?"))
		lbl.size = Vector2(CARD_W - 24, CARD_H - 24)
		lbl.position = Vector2(12, 12)
		lbl.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		lbl.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
		lbl.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
		lbl.add_theme_font_size_override("font_size", 28)
		card.add_child(lbl)

	# Borda (fica visível só quando focado).
	var border := ColorRect.new()
	border.name = "Border"
	border.color = Color(1, 1, 1, 0)
	border.size = Vector2(CARD_W, 4)
	border.position = Vector2(0, CARD_H + 8)
	card.add_child(border)

	card.modulate = DIM
	return card


# --------------------------------------------------------------------------- #
# Seleção / navegação
# --------------------------------------------------------------------------- #
func _select(idx: int, instant := false) -> void:
	if games.is_empty():
		return
	idx = clampi(idx, 0, games.size() - 1)
	selected = idx
	var g: Dictionary = games[idx]

	# Mata tweens de navegação anteriores: sem isso, segurar uma tecla de
	# direção (uso normal — o cooldown de 0.16s é menor que a duração de
	# 0.22s/0.30s dos tweens) empilha vários tweens concorrentes escrevendo
	# nas mesmas propriedades, causando flicker e um estado final inconsistente.
	for t_old in _nav_tweens:
		if t_old != null and is_instance_valid(t_old) and t_old.is_valid():
			t_old.kill()
	_nav_tweens.clear()

	# Anima cards.
	for i in cards.size():
		var card: Control = cards[i]
		var focused := i == idx
		var target_scale := Vector2(FOCUS_SCALE, FOCUS_SCALE) if focused else Vector2.ONE
		var target_mod := BRIGHT if focused else DIM
		var border: ColorRect = card.get_node("Border")
		if instant:
			card.scale = target_scale
			card.modulate = target_mod
			border.color = Color(0.4, 0.7, 1.0, 1.0) if focused else Color(1, 1, 1, 0)
		else:
			var t := create_tween().set_parallel(true)
			t.set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_OUT)
			t.tween_property(card, "scale", target_scale, 0.22)
			t.tween_property(card, "modulate", target_mod, 0.22)
			t.tween_property(border, "color",
				Color(0.4, 0.7, 1.0, 1.0) if focused else Color(1, 1, 1, 0), 0.22)
			_nav_tweens.append(t)

	# Centraliza a fileira no card selecionado.
	var target_x := 960.0 - (idx * (CARD_W + GAP) + CARD_W / 2.0)
	if instant:
		row.position.x = target_x
	else:
		var tr := create_tween().set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_OUT)
		tr.tween_property(row, "position:x", target_x, 0.30)
		_nav_tweens.append(tr)

	_update_info(g, instant)


func _update_info(g: Dictionary, instant: bool) -> void:
	# Título / logo.
	var logo := _load_texture(g.get("logo", ""))
	logo_rect.texture = logo
	logo_rect.visible = logo != null
	title_label.visible = logo == null
	title_label.text = str(g.get("title", ""))

	var launcher := str(g.get("launcher", "")).to_upper()
	tag_label.text = launcher

	# Hero art no fundo (crossfade).
	var hero := _load_texture(g.get("hero", ""))
	if hero == null:
		hero = _load_texture(g.get("cover", ""))
	if hero != null:
		bg_hero.texture = hero
		if instant:
			bg_hero.modulate = Color(0.55, 0.55, 0.6, 1.0)
		else:
			var t := create_tween()
			t.tween_property(bg_hero, "modulate:a", 1.0, 0.4)
	else:
		var t := create_tween()
		t.tween_property(bg_hero, "modulate:a", 0.0, 0.3)


func _process(delta: float) -> void:
	if overlay.visible:
		return
	nav_timer -= delta
	if nav_timer <= 0.0:
		if Input.is_action_pressed("ui_right"):
			if selected < games.size() - 1:
				_select(selected + 1)
			nav_timer = NAV_COOLDOWN
		elif Input.is_action_pressed("ui_left"):
			if selected > 0:
				_select(selected - 1)
			nav_timer = NAV_COOLDOWN


func _unhandled_input(event: InputEvent) -> void:
	if overlay.visible:
		if event.is_action_pressed("ui_cancel"):
			# Invalida qualquer await pendente de _launch_selected/_refresh_library:
			# sem isso, o timer da ação cancelada podia esconder o overlay de uma
			# ação NOVA iniciada logo em seguida.
			_overlay_token += 1
			overlay.visible = false
		return
	if event.is_action_pressed("ui_accept"):
		_launch_selected()
	elif event.is_action_pressed("ui_cancel"):
		pass  # reservado (voltar/menu)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_R:
		_refresh_library()
	elif event is InputEventJoypadButton and event.pressed and \
			event.button_index == JOY_BUTTON_START:
		_refresh_library()


# --------------------------------------------------------------------------- #
# Lançamento
# --------------------------------------------------------------------------- #
func _launch_selected() -> void:
	if games.is_empty():
		return
	var g: Dictionary = games[selected]
	var cmd: Array = g.get("launch_cmd", [])
	if cmd.is_empty():
		return
	var exe: String = str(cmd[0])
	var args := PackedStringArray()
	for i in range(1, cmd.size()):
		args.append(str(cmd[i]))

	_overlay_token += 1
	var token := _overlay_token
	overlay_label.text = "Iniciando\n" + str(g.get("title", ""))
	overlay.visible = true

	var pid := OS.create_process(exe, args)
	if pid <= 0:
		overlay_label.text = "Falha ao iniciar\n" + str(g.get("title", ""))
		await get_tree().create_timer(2.0).timeout
		if token == _overlay_token:
			overlay.visible = false
	else:
		# Some com o overlay depois de alguns segundos (o jogo assume a tela).
		await get_tree().create_timer(6.0).timeout
		if token == _overlay_token:
			overlay.visible = false


func _refresh_library() -> void:
	_overlay_token += 1
	var token := _overlay_token
	overlay_label.text = "Atualizando biblioteca…"
	overlay.visible = true
	var pid := OS.create_process("python3", PackedStringArray([index_path]))
	await get_tree().create_timer(2.0).timeout
	_load_library()
	_build_cards()
	if not games.is_empty():
		_select(clampi(selected, 0, games.size() - 1), true)
	if token == _overlay_token:
		overlay.visible = false


# --------------------------------------------------------------------------- #
# Dados / arte
# --------------------------------------------------------------------------- #
func _load_library() -> void:
	games = []
	if not FileAccess.file_exists(lib_path):
		return
	var f := FileAccess.open(lib_path, FileAccess.READ)
	if f == null:
		return
	var data = JSON.parse_string(f.get_as_text())
	f.close()
	if typeof(data) != TYPE_ARRAY:
		return
	# Ignora entradas que não são Dictionary: `var g: Dictionary = games[i]`
	# (usado em _build_cards/_select/_launch_selected) daria erro de tipo em
	# tempo de execução e interromperia a função no meio de um library.json
	# parcial/corrompido (ex.: index.py interrompido a meio da escrita).
	for entry in data:
		if typeof(entry) == TYPE_DICTIONARY:
			games.append(entry)
		else:
			push_warning("library.json: entrada inválida ignorada: %s" % str(entry))


var _tex_cache := {}

func _load_texture(path: String) -> Texture2D:
	if path == "" or path.begins_with("http"):
		return null  # URLs remotas (Heroic) ficam para depois
	if _tex_cache.has(path):
		return _tex_cache[path]
	if not FileAccess.file_exists(path):
		return null
	var img := Image.new()
	var err := img.load(path)
	if err != OK:
		return null
	var tex := ImageTexture.create_from_image(img)
	_tex_cache[path] = tex
	return tex


# Animação sutil de "respiração" no hero (efeito Ken Burns leve).
func _start_bg_animation() -> void:
	var t := create_tween().set_loops()
	t.set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)
	t.tween_property(bg_hero, "scale", Vector2(1.06, 1.06), 8.0)
	t.tween_property(bg_hero, "scale", Vector2(1.0, 1.0), 8.0)
