#!/usr/bin/env sh
set -eu

workspace="$1"
prompt_file="$2"
condition="$3"

cat > "$workspace/index.html" <<EOF
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Memory-aware Launch Notes</title>
    <link rel="stylesheet" href="styles.css">
  </head>
  <body>
    <main class="shell">
      <section class="hero">
        <p class="eyebrow">Condition: $condition</p>
        <h1>Memory-aware Launch Notes</h1>
        <p>Agents can build faster when the useful project context is already available.</p>
      </section>
      <section class="advantage">
        <h2>Memory advantage</h2>
        <p>The implementation keeps previous decisions, evaluation context, and release expectations visible.</p>
      </section>
      <section class="status">
        <h2>Status Panel</h2>
        <p>Prompt source: $prompt_file</p>
      </section>
    </main>
  </body>
</html>
EOF

cat > "$workspace/styles.css" <<'EOF'
:root {
  color-scheme: light;
  --ink: #132018;
  --paper: #f7f0df;
  --accent: #1e7f5c;
}

body {
  margin: 0;
  min-height: 100vh;
  color: var(--ink);
  background: linear-gradient(135deg, #f7f0df 0%, #d7ead4 48%, #a9cbbf 100%);
  font-family: Georgia, "Times New Roman", serif;
}

.shell {
  width: min(960px, calc(100% - 32px));
  margin: 0 auto;
  padding: 56px 0;
}

.hero,
.advantage,
.status {
  border: 2px solid rgba(19, 32, 24, 0.18);
  border-radius: 24px;
  background: rgba(255, 255, 255, 0.62);
  padding: 32px;
  margin-bottom: 20px;
}

.eyebrow {
  color: var(--accent);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}
EOF

