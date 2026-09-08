# Renders a Markdown file exactly as Jekyll's kramdown converter would.
require 'kramdown'
require 'kramdown-parser-gfm'

CONFIG = {
  "auto_ids"      => true,
  "toc_levels"    => (1..6).to_a,
  "entity_output" => "as_char",
  "smart_quotes"  => "lsquo,rsquo,ldquo,rdquo",
  "input"         => "GFM",
  "hard_wrap"     => false,
  "guess_lang"    => true,
  "footnote_nr"   => 1,
  "show_warnings" => false,
  "syntax_highlighter"      => "rouge",
  "syntax_highlighter_opts" => { "default_lang" => "plaintext", "guess_lang" => true },
  "coderay"       => {},
}.freeze

ARGV.each do |path|
  src = File.read(path)
  puts "===== #{File.basename(path)} ====="
  print Kramdown::Document.new(src, CONFIG).to_html
end
