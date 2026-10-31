# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class CountMinSketchTest < Minitest::Test
    def test_every_supported_shape_is_monomorphised
      [1024, 4096].product([4, 8]).each do |width, depth|
        sketch = CountMinSketch.new(width: width, depth: depth)

        assert_equal [width, depth], [sketch.width, sketch.depth]
      end
    end

    def test_an_unsupported_shape_is_rejected
      assert_raises(ArgumentError) { CountMinSketch.new(width: 2048, depth: 4) }
      assert_raises(ArgumentError) { CountMinSketch.new(width: 4096, depth: 5) }
      assert_raises(ArgumentError) { CountMinSketch.new(width: 16_384, depth: 8) }
      assert_raises(ArgumentError) { CountMinSketch.new(width: 4096) }
    end

    def test_counts_never_fall_below_the_truth_and_stay_inside_the_bound
      sketch = CountMinSketch.new(width: 1024, depth: 4)
      truth = Hash.new(0)
      5_000.times do |id|
        key = "path-#{id % 700}"
        truth[key] += 3
        sketch.add(key, count: 3)
      end

      assert_equal 15_000, sketch.total

      truth.each do |key, count|
        assert_operator sketch.count(key), :>=, count
        assert_operator sketch.count(key), :<=, count + (Math::E * sketch.total / 1024)
      end
    end

    def test_an_item_that_was_never_added_counts_zero
      assert_equal 0, CountMinSketch.new(width: 1024, depth: 4).count('nothing')
    end

    def test_dump_load_round_trips
      sketch = CountMinSketch.new(width: 1024, depth: 8)
      100.times { |id| sketch.add("path-#{id}", count: id) }
      blob = sketch.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 11 + (1024 * 8 * 4), blob.bytesize

      restored = CountMinSketch.load(blob)

      assert_equal [1024, 8, sketch.total], [restored.width, restored.depth, restored.total]
      assert_equal sketch.count('path-99'), restored.count('path-99')
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { CountMinSketch.load('') }
      assert_raises(ArgumentError) { CountMinSketch.load(HyperLogLog.new(registers: 1024).dump) }
      assert_raises(ArgumentError) { CountMinSketch.load("\xc1\x0b\x04".b + ("\x00".b * 8_192)) }
    end

    def test_merge_sums_the_counts
      left = CountMinSketch.new(width: 1024, depth: 4)
      right = CountMinSketch.new(width: 1024, depth: 4)
      left.add('path', count: 5)
      right.add('path', count: 7)

      assert_same left, left.merge(right)
      assert_equal 12, left.count('path')
      assert_equal 12, left.total
    end

    def test_merge_rejects_another_shape
      assert_raises(ArgumentError) do
        CountMinSketch.new(width: 1024, depth: 4).merge(CountMinSketch.new(width: 1024, depth: 8))
      end
    end

    def test_merge_with_the_receiver_doubles_it
      sketch = CountMinSketch.new(width: 1024, depth: 4)
      sketch.add('path', count: 5)

      sketch.merge(sketch)

      assert_equal 10, sketch.count('path')
      assert_equal 10, sketch.total
    end
  end
end
