# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class CountMinSketchTest < Minitest::Test
    # The largest count one counter holds.
    COUNTER_LIMIT = (2**32) - 1

    def test_exactly_the_supported_shapes_are_monomorphised
      assert_equal [[1024, 4096], [4, 8]], [CountMinSketch::WIDTHS, CountMinSketch::DEPTHS]
      CountMinSketch::WIDTHS.product(CountMinSketch::DEPTHS).each do |width, depth|
        sketch = CountMinSketch.new(width, depth)

        assert_equal [width, depth], [sketch.width, sketch.depth]
      end

      assert_raises(RangeError) { CountMinSketch.new(2048, 4) }
      assert_raises(RangeError) { CountMinSketch.new(4096, 5) }
      assert_raises(RangeError) { CountMinSketch.new(16_384, 8) }
      assert_raises(TypeError) { CountMinSketch.new(4096, 4.0) }
    end

    def test_counts_never_fall_below_the_truth_and_stay_inside_the_bound
      sketch = CountMinSketch.new(1024, 4)
      truth = Hash.new(0)
      5_000.times do |id|
        key = "path-#{id % 700}"
        truth[key] += 3

        assert_equal :added, sketch.add_count(key, 3)
      end

      assert_equal 15_000, sketch.total
      truth.each do |key, count|
        assert_operator sketch.estimate(key), :>=, count
        assert_operator sketch.estimate(key), :<=, count + (Math::E * sketch.total / 1024)
      end
    end

    def test_hashes_count_like_items
      sketch = CountMinSketch.new(1024, 4)

      assert_equal :added, sketch.add_hash_count(42, 7)
      assert_equal 7, sketch.estimate_hash(42)
      assert_equal 0, sketch.estimate('nothing')
    end

    def test_an_addition_past_a_full_counter_is_refused_and_changes_nothing
      sketch = CountMinSketch.new(1024, 4)

      assert_equal :added, sketch.add_count('path', COUNTER_LIMIT)
      before = sketch.encoded

      assert_equal :overflow, sketch.add('path')
      assert_equal :overflow, sketch.add_count('path', 2)
      assert_equal before, sketch.encoded
      assert_equal COUNTER_LIMIT, sketch.estimate('path')
      assert_raises(RangeError) { sketch.add_count('path', 2**32) }
    end

    def test_a_zero_count_is_refused_and_changes_nothing
      sketch = CountMinSketch.new(1024, 4)

      assert_equal :zero_count, sketch.add_count('path', 0)
      assert_equal :zero_count, sketch.add_hash_count(1, 0)
      assert_equal 0, sketch.total
      assert_raises(RangeError) { sketch.add_count('path', -1) }
      assert_raises(TypeError) { sketch.add_count('path', 0.9) }
    end

    def test_encoded_decode_round_trips
      sketch = CountMinSketch.new(1024, 8)
      100.times { |id| sketch.add_count("path-#{id}", id + 1) }
      blob = sketch.encoded

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 12 + (1024 * 8 * 4), blob.bytesize

      restored = CountMinSketch.decode(blob)

      assert_equal [1024, 8, sketch.total], [restored.width, restored.depth, restored.total]
      assert_equal blob, restored.encoded
    end

    def test_decode_answers_refusal_symbols
      assert_equal :truncated, CountMinSketch.decode('')
      assert_equal :wrong_structure, CountMinSketch.decode("\xa1".b * (12 + (1024 * 16)))
      assert_equal :wrong_shape, CountMinSketch.decode("\xc1\x0b\x04".b + ("\x00".b * 16_400))

      flagged = CountMinSketch.new(1024, 4).encoded
      flagged.setbyte(11, 1)

      assert_equal :malformed, CountMinSketch.decode(flagged)
    end

    def test_decode_into_reads_over_the_receiver_or_changes_nothing
      sketch = CountMinSketch.new(1024, 4)
      sketch.add('mine')
      source = CountMinSketch.new(1024, 4)
      source.add_count('theirs', 9)
      before = sketch.encoded

      assert_equal :wrong_shape, sketch.decode_into(CountMinSketch.new(1024, 8).encoded)
      assert_equal before, sketch.encoded
      assert_equal :decoded, sketch.decode_into(source.encoded)
      assert_equal 9, sketch.total
    end

    def test_merge_sums_the_counts
      left = CountMinSketch.new(1024, 4)
      right = CountMinSketch.new(1024, 4)
      left.add_count('path', 5)
      right.add_count('path', 7)

      assert_equal :merged, left.merge(right)
      assert_equal 12, left.estimate('path')
      assert_equal 12, left.total
    end

    def test_merge_refusals_change_nothing
      sketch = CountMinSketch.new(1024, 4)
      sketch.add_count('path', COUNTER_LIMIT)
      before = sketch.encoded
      one = CountMinSketch.new(1024, 4)
      one.add('path')

      assert_equal :overflow, sketch.merge(one)
      assert_equal :wrong_shape, sketch.merge(CountMinSketch.new(1024, 8))
      assert_equal before, sketch.encoded
    end

    def test_merge_with_the_receiver_doubles_it
      sketch = CountMinSketch.new(1024, 4)
      sketch.add_count('path', 5)

      assert_equal :merged, sketch.merge(sketch)
      assert_equal 10, sketch.estimate('path')
      assert_equal 10, sketch.total
    end
  end
end
