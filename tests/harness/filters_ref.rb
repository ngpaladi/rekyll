# Renders a template of filter expressions through real Jekyll + Liquid.
require 'jekyll'

source = File.expand_path(ARGV[1] || 'tests/fixtures/01-static')
config = Jekyll.configuration(
  'source' => source, 'destination' => '/tmp/rekyll-diff/filters-site',
  'quiet' => true, 'timezone' => 'UTC', 'time' => '2030-01-01 00:00:00 +0000',
  'url' => 'https://example.com', 'baseurl' => '/base'
)
site = Jekyll::Site.new(config)
site.reset
site.read

payload = site.site_payload
info = { registers: { site: site, page: {} }, strict_filters: false, strict_variables: false }

template_src = File.read(ARGV[0])
template_src.each_line do |line|
  line = line.chomp
  next if line.strip.empty? || line.start_with?('#')
  begin
    out = Liquid::Template.parse(line).render!(payload, info)
  rescue => e
    out = "ERROR(#{e.class})"
  end
  puts "#{line}\t=>\t#{out}"
end
