require 'date'
require 'psych'
def show(v)
  case v
  when NilClass then "Null"
  when TrueClass, FalseClass then "Bool(#{v})"
  when Integer then "Int(#{v})"
  when Float then (v.nan? ? "Float(NaN)" : "Float(#{v})")
  when String then "Str(#{v})"
  when Time then "Date(#{v.strftime('%Y-%m-%d %H:%M:%S %z')},time)"
  when Date then "Date(#{v.strftime('%Y-%m-%d')},date)"
  else "Other(#{v.class}:#{v})"
  end
end
File.readlines(ARGV[0], chomp: true).each do |line|
  begin
    doc = Psych.safe_load("v: #{line}", permitted_classes: [Date, Time], aliases: true)
    puts "#{line}\t#{show(doc['v'])}"
  rescue => e
    puts "#{line}\tERROR(#{e.class})"
  end
end
