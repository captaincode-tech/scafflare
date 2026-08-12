from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUTPUT = Path(__file__).resolve().parents[1] / "docs" / "assets" / "stackforge-demo.gif"
WIDTH, HEIGHT = 1120, 630
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"

FRAMES = [
    [
        ("$ stackforge init todo-api --non-interactive \\", "command"),
        ("    --framework express --architecture clean \\", "command"),
        ("    --database sqlite --vitest --biome --yes", "command"),
        ("", "plain"),
        ("preview", "accent"),
        ("  create   package.json                 (zod)", "success"),
        ("  create   src/server.ts                (express)", "success"),
        ("  create   src/modules/todos/service.ts (architecture-clean)", "success"),
        ("  create   .stackforge/lock.yaml        (stackforge-state)", "success"),
        ("done generated ./todo-api", "success"),
    ],
    [
        ("$ cd todo-api && npm install && npm run db:push", "command"),
        ("", "plain"),
        ("> todo-api@0.1.0 db:push", "plain"),
        ("> drizzle-kit push", "plain"),
        ("[✓] Changes applied", "success"),
        ("", "plain"),
        ("$ npm run build && NODE_ENV=production npm start", "command"),
        ("Server listening on http://localhost:3000", "success"),
    ],
    [
        ("$ curl http://localhost:3000/health", "command"),
        ('{"status":"ok"}', "success"),
        ("", "plain"),
        ("$ curl -X POST http://localhost:3000/todos \\", "command"),
        ("    -H 'content-type: application/json' \\", "command"),
        ("    -d '{\"title\":\"Ship StackForge\"}'", "command"),
        ('{"id":1,"title":"Ship StackForge","completed":false}', "success"),
        ("", "plain"),
        ("Fast, composable, production-minded backends.", "accent"),
    ],
]

COLORS = {
    "background": "#0b1020",
    "panel": "#111a30",
    "border": "#26365e",
    "plain": "#d5deef",
    "command": "#a8c7ff",
    "success": "#7ee2a8",
    "accent": "#78b7ff",
    "muted": "#8a99b8",
}


def make_frame(lines):
    image = Image.new("RGB", (WIDTH, HEIGHT), COLORS["background"])
    draw = ImageDraw.Draw(image)
    title_font = ImageFont.truetype(BOLD, 28)
    body_font = ImageFont.truetype(FONT, 22)
    draw.rounded_rectangle((35, 35, WIDTH - 35, HEIGHT - 35), radius=18, fill=COLORS["panel"], outline=COLORS["border"], width=2)
    draw.text((72, 72), "STACKFORGE  •  composable backend scaffolding", font=title_font, fill=COLORS["accent"])
    draw.text((72, 115), "terminal demo", font=body_font, fill=COLORS["muted"])
    y = 170
    for line, kind in lines:
        draw.text((72, y), line, font=body_font, fill=COLORS[kind])
        y += 39
    return image


images = [make_frame(lines) for lines in FRAMES]
images[0].save(OUTPUT, save_all=True, append_images=images[1:], duration=[2500, 2200, 2800], loop=0, optimize=True)
print(OUTPUT)
