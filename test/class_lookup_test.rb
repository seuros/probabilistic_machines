# frozen_string_literal: true

require 'test_helper'
require 'open3'
require 'rbconfig'

module ProbabilisticMachines
  # The classes of the sketches the extension wraps (HyperLogLog,
  # CountMinSketch, TDigest, CuckooFilter, HeavyHitters) are fixed when it
  # loads. Nothing a caller does later, redefining a constant or
  # `Object.const_get`, reaches a sketch being built. Each case runs in a fresh
  # process: the class is fixed once per process, at load.
  class ClassLookupTest < Minitest::Test
    LIB = File.expand_path('../lib', __dir__)

    # Each sketch class with the arguments of its smallest `new`, and every
    # sketch built both ways (`new`, `decode`) as `name new_class decode_class`.
    PRELUDE = <<~RUBY
      require 'probabilistic_machines'
      P = ProbabilisticMachines
      ARGS = {
        HyperLogLog: [1024], CountMinSketch: [P::CountMinSketch::WIDTHS.first, P::CountMinSketch::DEPTHS.first],
        TDigest: [100], CuckooFilter: [P::CuckooFilter::BUCKETS.first], HeavyHitters: [P::HeavyHitters::COUNTERS.first]
      }.freeze
      CLASSES = ARGS.to_h { |name, _| [name, P.const_get(name)] }.freeze
      def build(classes = CLASSES)
        ARGS.map do |name, args|
          sketch = classes.fetch(name).new(*args)
          decoded = classes.fetch(name).decode(sketch.encoded)
          [name, sketch, decoded]
        end
      end
    RUBY

    # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
    def isolated(body)
      out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
      assert status.success?, "exited #{status.exitstatus}: #{err}"
      out.lines.last&.chomp
    end

    # Bug: a sketch constant redefined before the first sketch made `new` and
    # `decode` answer an instance of the new class, with none of the sketch's
    # methods.
    def test_a_redefined_constant_does_not_change_the_sketch_class
      assert_equal 'true true true true true', isolated(<<~RUBY)
        CLASSES.each_key do |name|
          P.send(:remove_const, name)
          P.const_set(name, Class.new)
        end
        puts(build.map do |name, sketch, decoded|
          original = CLASSES.fetch(name)
          [sketch, decoded].all? { it.instance_of?(original) } && decoded.encoded == sketch.encoded
        end.join(' '))
      RUBY
    end

    # Bug: building the first sketch ran `Object.const_get`, a caller's code in
    # the middle of a native call. No Ruby runs while a sketch is built now.
    def test_building_a_sketch_runs_no_const_get
      assert_equal '0 HyperLogLog CountMinSketch TDigest CuckooFilter HeavyHitters', isolated(<<~RUBY)
        hits = 0
        Object.define_singleton_method(:const_get) do |*args|
          hits += 1
          super(*args)
        end
        built = build
        names = built.map { |name, sketch, decoded| [sketch, decoded].map { it.class.name.split('::').last }.uniq }
        puts [hits, *names.flatten].join(' ')
      RUBY
    end

    # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
    # extension: an uncatchable fatal, and the process exited.
    def test_a_raising_const_get_does_not_kill_the_process
      assert_equal '5 5', isolated(<<~RUBY)
        Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran mid-call' }
        built = build
        puts [built.size, built.count { |_, sketch, decoded| decoded.encoded == sketch.encoded }].join(' ')
      RUBY
    end
  end
end
