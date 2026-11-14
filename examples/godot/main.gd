extends Node2D

const CHECK_SEED := 77
const CHECK_CAP := 240 * 60 * 10
const TICKS_PER_FRAME := 4
const BOT_LEVEL := 3
const MARGIN := 60.0

var game: RefCounted
var scale_px := 1.0
var half := Vector2.ONE


func _ready() -> void:
	if not ClassDB.class_exists("FeintMatch"):
		printerr("FEINT the extension did not load")
		get_tree().quit(3)
		return
	game = ClassDB.instantiate("FeintMatch")
	if "--check" in OS.get_cmdline_user_args():
		if game.start(1, 9, 3) or game.start(1, 1, 0) or game.start(-1, 1, 3):
			printerr("FEINT a level or seed out of range was accepted")
			get_tree().quit(6)
			return
		if not game.start(5, 0, 3):
			get_tree().quit(4)
			return
		for i in 2400:
			if game.step(0.1 * sin(i * 0.01), 0.2) < 0:
				printerr("FEINT the human paddle failed at tick %d" % i)
				get_tree().quit(7)
				return
		if not game.start(CHECK_SEED, 1, 5):
			get_tree().quit(4)
			return
		var line: String = game.play_out(CHECK_CAP)
		print("FEINT " + line)
		get_tree().quit(0 if line.begins_with("score") else 5)
		return
	if not game.start(randi() % 1000000, 0, BOT_LEVEL):
		get_tree().quit(4)
		return
	half = Vector2(game.layout(0), game.layout(1))
	var view := get_viewport_rect().size
	scale_px = minf((view.x - 2.0 * MARGIN) / (2.0 * half.x), (view.y - 2.0 * MARGIN) / (2.0 * half.y))


func to_screen(p: Vector2) -> Vector2:
	return get_viewport_rect().size / 2.0 + Vector2(p.x, -p.y) * scale_px


func to_table(s: Vector2) -> Vector2:
	var p := (s - get_viewport_rect().size / 2.0) / scale_px
	return Vector2(clampf(p.x, -half.x, half.x), clampf(-p.y, -half.y, half.y))


func _physics_process(_delta: float) -> void:
	if game == null or game.finished():
		return
	var aim := to_table(get_viewport().get_mouse_position())
	for i in TICKS_PER_FRAME:
		if game.step(aim.x, aim.y) != 0:
			break
	queue_redraw()


func _draw() -> void:
	if game == null:
		return
	var top_left := to_screen(Vector2(-half.x, half.y))
	draw_rect(Rect2(top_left, Vector2(2.0 * half.x, 2.0 * half.y) * scale_px), Color(0.1, 0.16, 0.24))
	var mouth: float = game.layout(2)
	for end in [-1.0, 1.0]:
		draw_line(to_screen(Vector2(end * half.x, mouth)), to_screen(Vector2(end * half.x, -mouth)), Color(0.9, 0.3, 0.3), 6.0)
	draw_line(to_screen(Vector2(0, half.y)), to_screen(Vector2(0, -half.y)), Color(0.3, 0.4, 0.5), 2.0)
	draw_circle(to_screen(Vector2(game.position(0), game.position(1))), game.layout(3) * scale_px, Color.WHITE)
	draw_circle(to_screen(Vector2(game.position(4), game.position(5))), game.layout(4) * scale_px, Color(0.3, 0.8, 1.0))
	draw_circle(to_screen(Vector2(game.position(8), game.position(9))), game.layout(4) * scale_px, Color(1.0, 0.5, 0.3))
	var text := "you %d : %d bot level %d" % [game.score(0), game.score(1), BOT_LEVEL]
	if game.finished():
		text += " — over"
	draw_string(ThemeDB.fallback_font, Vector2(MARGIN, 36), text, HORIZONTAL_ALIGNMENT_LEFT, -1, 22)
