# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  # What the native boundary holds: a frozen sketch changes through no method,
  # and the Data values are sealed, with the leaks sealing cannot stop.
  class BoundaryTest < Minitest::Test
    def test_a_frozen_sketch_refuses_every_mutating_call
      sketches = {
        HyperLogLog.new(1024) => [->(s) { s.add('a') }, ->(s) { s.merge(HyperLogLog.new(1024)) }],
        CountMinSketch.new(1024, 4) => [->(s) { s.add('a') }, ->(s) { s.add_count('a', 2) },
                                        ->(s) { s.add_hash_count(1, 2) },
                                        ->(s) { s.merge(CountMinSketch.new(1024, 4)) },
                                        ->(s) { s.decode_into(CountMinSketch.new(1024, 4).encoded) }],
        CuckooFilter.new(1024) => [->(s) { s.insert('a') }, ->(s) { s.insert_hash(1) },
                                   ->(s) { s.remove('a') },
                                   ->(s) { s.decode_into(CuckooFilter.new(1024).encoded) }],
        HeavyHitters.new(16) => [->(s) { s.add('a') }, ->(s) { s.add_count('a', 2) },
                                 ->(s) { s.merge(HeavyHitters.new(16)) },
                                 ->(s) { s.decode_into(HeavyHitters.new(16).encoded) }],
        TDigest.new(100) => [->(s) { s.add(1.0) }, ->(s) { s.add_weighted(1.0, 2.0) }, lambda(&:compress),
                             ->(s) { s.merge(TDigest.new(100)) }]
      }
      sketches.each do |sketch, mutations|
        before = sketch.encoded
        sketch.freeze
        mutations.each { |mutate| assert_raises(FrozenError, sketch.inspect) { mutate.call(sketch) } }
        assert_equal before, sketch.encoded
      end
    end

    # Frozen is checked before any argument is read, as Ruby's own methods do.
    def test_a_frozen_sketch_given_a_bad_argument_raises_frozen_error
      bad = {
        HyperLogLog.new(1024) => %i[add add_hash merge decode_into],
        CountMinSketch.new(1024, 4) => %i[add add_count add_hash_count merge decode_into],
        CuckooFilter.new(1024) => %i[insert insert_hash remove remove_hash decode_into],
        HeavyHitters.new(16) => %i[add add_count merge decode_into],
        TDigest.new(100) => %i[add add_weighted merge]
      }
      bad.each do |sketch, names|
        sketch.freeze
        names.each do |name|
          arguments = Array.new(sketch.method(name).arity, :bad)
          assert_raises(FrozenError, "#{sketch.class}##{name}") { sketch.public_send(name, *arguments) }
        end
      end
    end

    # Sealing stops accidents, not reflection: Ruby binds a named subclass's
    # constant before `inherited` raises, so the class leaks. Both forms raise.
    def test_values_are_sealed_and_a_named_subclass_leaks_its_constant
      [Hitter, Centroid].each { |value| assert_raises(TypeError) { Class.new(value) } }
      assert_raises(TypeError) { self.class.module_eval('class LeakedHitter < Hitter; end', __FILE__, __LINE__) }
      assert self.class.const_defined?(:LeakedHitter, false), 'the constant is bound before inherited raises'
    end

    # The binding builds `Hitter` and `Centroid` and reads neither back: no
    # method takes one, so a forged value (a leaked subclass skipping the check,
    # or a Marshal frame edited past it) has no way into a sketch.
    def test_a_forged_value_has_no_way_into_a_sketch
      edited = Marshal.load(Marshal.dump(Centroid.new(1.0, 2.0)).sub("f\x062".b, "f\x060".b))
      assert_in_delta 0.0, edited.weight
      digest = TDigest.new(100)
      assert_raises(TypeError) { digest.add(edited) }
      assert_raises(TypeError) { digest.add_weighted(1.0, edited) }
      assert_raises(TypeError) { HeavyHitters.new(16).add(Hitter.new('k', 2, 1)) }
      assert digest.empty?
    end

    # A merge partner is checked natively for exactly the sketch's class (no
    # subclass instance), against the class pinned at init: compaction moves
    # nothing it holds.
    def test_a_merge_partner_is_exactly_the_sketch_class
      GC.verify_compaction_references(expand_heap: true, toward: :empty)
      partners = { HyperLogLog.new(1024) => HyperLogLog.new(1024), CountMinSketch.new(1024, 4) => CountMinSketch.new(1024, 4),
                   HeavyHitters.new(16) => HeavyHitters.new(16), TDigest.new(100) => TDigest.new(100) }
      partners.each do |sketch, partner|
        assert_equal :merged, sketch.merge(partner)
        error = assert_raises(TypeError) { sketch.merge(CuckooFilter.new(1024)) }
        assert_equal "other must be a #{sketch.class}, not ProbabilisticMachines::CuckooFilter", error.message
        error = assert_raises(TypeError) { sketch.merge(:bad) }
        assert_equal "other must be a #{sketch.class}, not Symbol", error.message
      end
    end

    # Frames and items are exactly a String, their bytes copied natively: a
    # String subclass instance and an object answering to_str are TypeError,
    # and a redefined #bytes, #to_s or #to_str on a String changes nothing.
    def test_frames_and_items_are_exactly_a_string
      to_str = Object.new.tap { |object| def object.to_str = 'a' }
      bad = [Class.new(String).new('a'), to_str, :a, nil]
      frames = { HyperLogLog => HyperLogLog.new(1024), CountMinSketch => CountMinSketch.new(1024, 4),
                 CuckooFilter => CuckooFilter.new(1024), HeavyHitters => HeavyHitters.new(16), TDigest => TDigest.new(100) }
      frames.each do |klass, sketch|
        bad.each do |value|
          assert_equal 'bytes must be a String', assert_raises(TypeError) { klass.decode(value) }.message
          next unless sketch.respond_to?(:decode_into)

          assert_equal 'bytes must be a String', assert_raises(TypeError) { sketch.decode_into(value) }.message
        end
      end
      hll = HyperLogLog.new(1024)
      cms = CountMinSketch.new(1024, 4)
      filter = CuckooFilter.new(1024)
      items = [-> { hll.add(it) }, -> { cms.add(it) }, -> { cms.add_count(it, 2) }, -> { cms.estimate(it) },
               -> { filter.insert(it) }, -> { filter.contains?(it) }, -> { filter.remove(it) }]
      bad.each do |value|
        items.each { |call| assert_equal 'item must be a String', assert_raises(TypeError) { call.call(value) }.message }
      end
      assert_equal [true, 0, true], [hll.empty?, cms.total, filter.empty?]
      liar = +'a'
      def liar.bytes = 'b'.bytes
      def liar.to_s = 'b'
      def liar.to_str = 'b'
      assert_equal [:added, :inserted], [cms.add(liar), filter.insert(liar)]
      assert_equal [1, 0], [cms.estimate('a'), cms.estimate('b')]
      assert filter.contains?('a')
      frame = +cms.encoded
      def frame.bytes = []
      assert_instance_of CountMinSketch, CountMinSketch.decode(frame)
    end

    # The classes are resolved at init and pinned, so a compaction moves none
    # of them: every path that builds a value still works after it.
    def test_every_value_path_survives_gc_compact
      skip 'GC.compact is not available' unless GC.respond_to?(:compact)
      GC.compact
      table = HeavyHitters.new(16)
      table.add_count('a', 2)
      assert_equal [Hitter.new('a', 2, 0)], [table.top, *table.ranked, *table.hitters].uniq
      digest = TDigest.new(100)
      digest.add(1.0)
      assert_equal [Centroid.new(1.0, 1.0)], digest.centroids
    end

    # The key a Hitter holds is a frozen copy: changing the caller's String
    # afterwards changes no value.
    def test_a_hitter_key_is_a_frozen_copy
      key = +'abc'
      hitter = Hitter.new(key, 2, 1)
      key << 'd'

      assert_equal 'abc', hitter.key
      assert_predicate hitter.key, :frozen?
      refute_predicate key, :frozen?
    end
  end
end
