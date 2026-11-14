# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class CuckooFilterTest < Minitest::Test
    def test_exactly_the_supported_bucket_counts_are_monomorphised
      assert_equal [1024, 4096, 16_384, 65_536], CuckooFilter::BUCKETS
      CuckooFilter::BUCKETS.each do |buckets|
        filter = CuckooFilter.new(buckets)

        assert_equal buckets, filter.buckets
        assert_equal buckets * 4, filter.capacity
      end

      [2, 512, 2048, 131_072].each do |buckets|
        assert_raises(RangeError) { CuckooFilter.new(buckets) }
      end
      assert_raises(TypeError) { CuckooFilter.new(nil) }
    end

    def test_an_inserted_item_is_never_missing
      filter = CuckooFilter.new(4096)
      keys = Array.new(2_000) { |id| "device-#{id}" }
      keys.each { |key| assert_equal :inserted, filter.insert(key) }

      assert_equal 2_000, filter.size
      keys.each { |key| assert filter.contains?(key) }
    end

    def test_hashes_are_items_of_their_own
      filter = CuckooFilter.new(1024)

      assert_equal :inserted, filter.insert_hash(99)
      assert filter.contains_hash?(99)
      assert_equal :cleared, filter.remove_hash(99)
      refute filter.contains_hash?(99)
      assert_equal :not_held, filter.remove_hash(99)
    end

    def test_remove_clears_one_copy_and_a_repeat_is_not_held
      filter = CuckooFilter.new(1024)
      filter.insert('device-9')
      filter.insert('device-9')

      assert_equal :cleared, filter.remove('device-9')
      assert filter.contains?('device-9')
      assert_equal :cleared, filter.remove('device-9')
      refute filter.contains?('device-9')
      assert_predicate filter, :empty?

      before = filter.encoded

      assert_equal :not_held, filter.remove('device-9')
      assert_equal before, filter.encoded
    end

    def test_a_full_filter_refuses_and_changes_nothing
      filter = CuckooFilter.new(1024)
      id = 0
      id += 1 while filter.insert("device-#{id}") == :inserted
      before = filter.encoded

      assert_equal :full, filter.insert("device-#{id}")
      assert_equal before, filter.encoded
      assert_operator filter.size, :<=, filter.capacity
    end

    def test_encoded_decode_round_trips
      filter = CuckooFilter.new(1024)
      100.times { |id| filter.insert("device-#{id}") }
      blob = filter.encoded

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 2 + (1024 * 4), blob.bytesize

      restored = CuckooFilter.decode(blob)

      assert_equal [1024, 100], [restored.buckets, restored.size]
      100.times { |id| assert restored.contains?("device-#{id}") }
    end

    def test_decode_answers_refusal_symbols
      assert_equal :truncated, CuckooFilter.decode('')
      assert_equal :wrong_structure, CuckooFilter.decode("\xa1\x0a".b + ("\x00".b * 4_096))
      assert_equal :wrong_shape, CuckooFilter.decode("\xf1\x0b".b + ("\x00".b * 8_192))
    end

    def test_decode_into_reads_over_the_receiver_or_changes_nothing
      filter = CuckooFilter.new(1024)
      filter.insert('mine')
      before = filter.encoded

      assert_equal :wrong_shape, filter.decode_into(CuckooFilter.new(4096).encoded)
      assert_equal before, filter.encoded
      assert_equal :decoded, filter.decode_into(CuckooFilter.new(1024).encoded)
      assert_predicate filter, :empty?
    end
  end
end
