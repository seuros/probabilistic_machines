# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class HyperLogLogTest < Minitest::Test
    def test_exactly_the_supported_register_counts_are_monomorphised
      assert_equal [1024, 4096, 16_384, 65_536], HyperLogLog::REGISTERS
      assert_predicate HyperLogLog::REGISTERS, :frozen?
      HyperLogLog::REGISTERS.each do |registers|
        assert_equal registers, HyperLogLog.new(registers).registers
      end

      [16, 512, 2048, 131_072, -1].each do |registers|
        assert_raises(RangeError) { HyperLogLog.new(registers) }
      end
      assert_raises(TypeError) { HyperLogLog.new(1024.0) }
      assert_raises(TypeError) { HyperLogLog.new('1024') }
      assert_raises(TypeError) { HyperLogLog.allocate }
    end

    def test_the_estimate_lands_inside_the_standard_error
      sketch = HyperLogLog.new(4096)
      50_000.times { |id| assert_nil sketch.add("user-#{id}") }

      error = (sketch.estimate - 50_000).abs / 50_000

      assert_operator error, :<, 3 * sketch.standard_error
      assert_in_delta 1.04 / Math.sqrt(4096), sketch.standard_error, 1e-15
    end

    def test_hashes_count_as_their_own_items
      sketch = HyperLogLog.new(1024)
      random = Random.new(20_260_101)
      hashes = Array.new(1_000) { random.rand(1 << 64) }
      hashes.each { |hash| sketch.add_hash(hash) }

      refute_predicate sketch, :empty?
      assert_in_delta 1_000, sketch.estimate, 1_000 * 3 * sketch.standard_error

      estimate = sketch.estimate
      hashes.each { |hash| sketch.add_hash(hash) }

      assert_equal estimate, sketch.estimate
      assert_raises(RangeError) { sketch.add_hash(-1) }
      assert_raises(TypeError) { sketch.add_hash(1.5) }
    end

    def test_encoded_decode_round_trips
      sketch = HyperLogLog.new(16_384)
      10_000.times { |id| sketch.add("user-#{id}") }
      blob = sketch.encoded

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 16_386, blob.bytesize

      restored = HyperLogLog.decode(blob)

      assert_equal 16_384, restored.registers
      assert_equal sketch.estimate, restored.estimate
    end

    def test_decode_answers_refusal_symbols_and_never_raises
      assert_equal :truncated, HyperLogLog.decode('')
      assert_equal :wrong_structure, HyperLogLog.decode(CuckooFilter.new(1024).encoded.byteslice(0, 1026))
      assert_equal :wrong_shape, HyperLogLog.decode("\xa1\x0b".b + ("\x00".b * 2_048))
      assert_equal :malformed, HyperLogLog.decode("\xa1\x0a".b + ("\x40".b * 1_024))
      assert_raises(TypeError) { HyperLogLog.decode(nil) }
    end

    def test_decode_into_reads_over_the_receiver_or_changes_nothing
      sketch = HyperLogLog.new(1024)
      source = HyperLogLog.new(1024)
      500.times { |id| source.add("user-#{id}") }
      sketch.add('only')
      before = sketch.encoded

      assert_equal :wrong_shape, sketch.decode_into(HyperLogLog.new(4096).encoded)
      assert_equal :truncated, sketch.decode_into(source.encoded.byteslice(0, 10))
      assert_equal before, sketch.encoded

      assert_equal :decoded, sketch.decode_into(source.encoded)
      assert_equal source.encoded, sketch.encoded
    end

    def test_merge_unions_the_two_streams
      left = HyperLogLog.new(4096)
      right = HyperLogLog.new(4096)
      10_000.times { |id| left.add("left-#{id}") }
      10_000.times { |id| right.add("right-#{id}") }

      assert_equal :merged, left.merge(right)

      error = (left.estimate - 20_000).abs / 20_000

      assert_operator error, :<, 3 * left.standard_error
    end

    def test_merge_with_the_receiver_is_idempotent
      sketch = HyperLogLog.new(1024)
      1_000.times { |id| sketch.add("user-#{id}") }
      before = sketch.encoded

      assert_equal :merged, sketch.merge(sketch)
      assert_equal before, sketch.encoded
    end

    def test_merge_of_another_register_count_is_refused_and_changes_nothing
      sketch = HyperLogLog.new(1024)
      sketch.add('x')
      before = sketch.encoded
      other = HyperLogLog.new(4096)
      other.add('y')

      assert_equal :wrong_shape, sketch.merge(other)
      assert_equal before, sketch.encoded
      assert_raises(TypeError) { sketch.merge(CuckooFilter.new(1024)) }
    end

    def test_a_frozen_sketch_refuses_mutation
      sketch = HyperLogLog.new(1024).freeze

      assert_raises(FrozenError) { sketch.add('x') }
      assert_raises(FrozenError) { sketch.merge(HyperLogLog.new(1024)) }
      assert_predicate sketch, :empty?
    end
  end
end
