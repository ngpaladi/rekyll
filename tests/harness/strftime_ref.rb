require 'time'
times = [
  Time.new(2020, 1, 2, 3, 4, 5, 0),
  Time.new(2021, 12, 31, 23, 59, 59, -5*3600),
  Time.new(2024, 2, 29, 12, 0, 0, 5.5*3600),
  Time.new(1999, 7, 4, 0, 0, 0, 0),
]
File.readlines(ARGV[0], chomp: true).each do |fmt|
  times.each_with_index do |t, i|
    puts "#{i}\t#{fmt}\t#{t.strftime(fmt)}"
  end
end
