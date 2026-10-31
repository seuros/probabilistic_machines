# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class HyperLogLogTest < Minitest::Test
    def test_every_supported_register_count_is_monomorphised
      [1024, 4096, 16384, 65536].each do |registers|
        assert_equal registers, HyperLogLog.new(registers: registers).registers
      end
    end

    def test_an_unsupported_register_count_is_rejected
      [16, 512, 2048, 131_072].each do |registers|
        error = assert_raises(ArgumentError) { HyperLogLog.new(registers: registers) }

        assert_match(/#{registers} is not supported/, error.message)
      end
    end

    def test_a_missing_register_count_is_rejected
      assert_raises(ArgumentError) { HyperLogLog.new }
    end

    def test_the_estimate_lands_inside_the_standard_error
      sketch = HyperLogLog.new(registers: 4096)
      50_000.times { |id| sketch.add("user-#{id}") }

      error = (sketch.estimate - 50_000).abs / 50_000

      assert_operator error, :<, 3 * sketch.standard_error
    end

    def test_hashes_count_as_their_own_items
      sketch = HyperLogLog.new(registers: 1024)
      random = Random.new(20_260_101)
      hashes = Array.new(1_000) { random.rand(1 << 64) }
      hashes.each { |hash| sketch.add_hash(hash) }

      refute_predicate sketch, :empty?
      assert_in_delta 1_000, sketch.estimate, 1_000 * 3 * sketch.standard_error

      estimate = sketch.estimate
      hashes.each { |hash| sketch.add_hash(hash) }

      assert_in_delta estimate, sketch.estimate, 0.000_1
    end

    def test_dump_load_round_trips
      sketch = HyperLogLog.new(registers: 16_384)
      10_000.times { |id| sketch.add("user-#{id}") }
      blob = sketch.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 16_386, blob.bytesize

      restored = HyperLogLog.load(blob)

      assert_equal 16_384, restored.registers
      assert_in_delta sketch.estimate, restored.estimate, 0.000_1
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { HyperLogLog.load('') }
      assert_raises(ArgumentError) { HyperLogLog.load(TDigest.new(compression: 100).dump) }
      assert_raises(ArgumentError) { HyperLogLog.load("\xa1\x0b".b + ("\x00".b * 2_048)) }
    end

    def test_merge_unions_the_two_streams
      left = HyperLogLog.new(registers: 4096)
      right = HyperLogLog.new(registers: 4096)
      10_000.times { |id| left.add("left-#{id}") }
      10_000.times { |id| right.add("right-#{id}") }

      assert_same left, left.merge(right)

      error = (left.estimate - 20_000).abs / 20_000

      assert_operator error, :<, 3 * left.standard_error
    end

    def test_merge_is_idempotent_on_the_receiver
      sketch = HyperLogLog.new(registers: 1024)
      1_000.times { |id| sketch.add("user-#{id}") }
      before = sketch.estimate

      sketch.merge(sketch)

      assert_in_delta before, sketch.estimate, 0.000_1
    end

    def test_merge_rejects_another_register_count
      assert_raises(ArgumentError) do
        HyperLogLog.new(registers: 1024).merge(HyperLogLog.new(registers: 4096))
      end
    end
  end
end
