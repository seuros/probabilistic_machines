# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class CuckooFilterTest < Minitest::Test
    def test_every_supported_bucket_count_is_monomorphised
      [1024, 4096, 16_384, 65_536].each do |buckets|
        filter = CuckooFilter.new(buckets: buckets)

        assert_equal buckets, filter.buckets
        assert_equal buckets * 4, filter.capacity
      end
    end

    def test_an_unsupported_bucket_count_is_rejected
      [2, 512, 2048, 131_072].each do |buckets|
        assert_raises(ArgumentError) { CuckooFilter.new(buckets: buckets) }
      end
      assert_raises(ArgumentError) { CuckooFilter.new }
    end

    def test_an_inserted_item_is_never_missing
      filter = CuckooFilter.new(buckets: 4096)
      keys = Array.new(2_000) { |id| "device-#{id}" }
      keys.each { |key| assert filter.insert(key) }

      assert_equal 2_000, filter.size
      keys.each { |key| assert_includes filter, key }
    end

    def test_contains_is_the_same_question_as_include
      filter = CuckooFilter.new(buckets: 1024)
      filter.insert('device-9')

      assert_operator filter, :contains?, 'device-9'
      refute_operator filter, :contains?, 'device-8'
    end

    def test_remove_forgets_the_item
      filter = CuckooFilter.new(buckets: 1024)
      filter.insert('device-9')

      assert filter.remove('device-9')
      refute_includes filter, 'device-9'
      assert_predicate filter, :empty?
      refute filter.remove('device-9')
    end

    def test_a_full_filter_raises
      filter = CuckooFilter.new(buckets: 1024)

      assert_raises(Full) do
        10_000.times { |id| filter.insert("device-#{id}") }
      end
      assert_operator filter.size, :<=, filter.capacity
    end

    def test_dump_load_round_trips
      filter = CuckooFilter.new(buckets: 1024)
      100.times { |id| filter.insert("device-#{id}") }
      blob = filter.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 2 + (1024 * 4), blob.bytesize

      restored = CuckooFilter.load(blob)

      assert_equal [1024, 100], [restored.buckets, restored.size]
      100.times { |id| assert_includes restored, "device-#{id}" }
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { CuckooFilter.load('') }
      assert_raises(ArgumentError) { CuckooFilter.load(HyperLogLog.new(registers: 1024).dump) }
      assert_raises(ArgumentError) { CuckooFilter.load("\xf1\x0b".b + ("\x00".b * 8_192)) }
    end
  end
end
