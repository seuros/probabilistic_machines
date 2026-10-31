# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class HeavyHittersTest < Minitest::Test
    def test_every_supported_counter_count_is_monomorphised
      [16, 64, 256].each do |counters|
        assert_equal counters, HeavyHitters.new(k: counters).k
      end
    end

    def test_an_unsupported_counter_count_is_rejected
      [1, 32, 128, 1024].each do |counters|
        assert_raises(ArgumentError) { HeavyHitters.new(k: counters) }
      end
      assert_raises(ArgumentError) { HeavyHitters.new }
    end

    def test_the_dominant_key_is_tracked_with_its_own_string
      table = HeavyHitters.new(k: 16)
      5_000.times { |id| table.add("ip-#{id}") }
      1_000.times { table.add('ip-hot', count: 2) }

      assert_equal 7_000, table.observed
      assert_equal 16, table.size

      top = table.top(1).first

      assert_instance_of Hitter, top
      assert_predicate top, :frozen?
      assert_equal 'ip-hot', top.key
      assert_operator top.count, :>=, 2_000
      assert_operator top.count - top.overestimate, :<=, 2_000
    end

    def test_a_key_is_tracked_once_it_beats_the_counter_share
      table = HeavyHitters.new(k: 64)
      10_000.times { |id| table.add("ip-#{id % 1_000}") }
      300.times { table.add('ip-frequent') }

      assert_operator 300, :>, table.observed / 64

      tracked = table.ranked.find { |hitter| hitter.key == 'ip-frequent' }

      refute_nil tracked
      assert_operator tracked.count, :>=, 300
      assert_operator tracked.count - tracked.overestimate, :<=, 300
    end

    def test_an_untracked_key_counts_zero
      assert_equal 0, HeavyHitters.new(k: 16).count('ip-1')
    end

    def test_top_is_ordered_and_bounded_by_the_request
      table = HeavyHitters.new(k: 64)
      %w[a b c].each_with_index { |key, index| table.add(key, count: 10 - index) }

      assert_equal %w[a b c], table.top(5).map(&:key)
      assert_equal %w[a b], table.top(2).map(&:key)
      assert_equal 3, table.ranked.size
    end

    def test_names_survive_eviction_churn
      table = HeavyHitters.new(k: 16)
      5_000.times { |id| table.add("ip-#{id}") }

      assert_equal 16, table.ranked.size
      table.ranked.each { |hitter| assert_match(/\Aip-\d+\z/, hitter.key) }
    end

    def test_merge_folds_both_tables
      left = HeavyHitters.new(k: 16)
      right = HeavyHitters.new(k: 16)
      100.times { left.add('ip-1') }
      50.times { right.add('ip-2') }

      assert_same left, left.merge(right)
      assert_equal 150, left.observed
      assert_equal 100, left.count('ip-1')
      assert_equal 50, left.count('ip-2')
      assert_equal %w[ip-1 ip-2], left.top(2).map(&:key)
    end

    def test_dump_load_round_trips_with_the_original_keys
      table = HeavyHitters.new(k: 16)
      5_000.times { |id| table.add("ip-#{id}") }
      1_000.times { table.add('ip-hot', count: 2) }
      blob = table.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 13 + (16 * 24), HeavyHitters.new(k: 16).dump.bytesize

      restored = HeavyHitters.load(blob)

      assert_equal [16, table.observed, table.size], [restored.k, restored.observed, restored.size]
      assert_equal table.ranked.map(&:key), restored.ranked.map(&:key)
      assert_equal table.ranked.map(&:count), restored.ranked.map(&:count)
      assert_equal table.ranked.map(&:overestimate), restored.ranked.map(&:overestimate)
      assert_equal table.count('ip-hot'), restored.count('ip-hot')
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { HeavyHitters.load('') }
      assert_raises(ArgumentError) { HeavyHitters.load(HyperLogLog.new(registers: 1024).dump) }
      assert_raises(ArgumentError) { HeavyHitters.load("\xb1\x20\x00".b + ("\x00".b * 800)) }

      blob = HeavyHitters.new(k: 16).dump

      assert_raises(ArgumentError) { HeavyHitters.load(blob.byteslice(0, 100)) }
    end

    def test_load_rejects_a_blob_whose_keys_went_missing
      table = HeavyHitters.new(k: 16)
      table.add('ip-1')

      assert_raises(ArgumentError) { HeavyHitters.load(table.dump.byteslice(0, 13 + (16 * 24))) }
    end

    def test_merge_rejects_another_counter_count
      assert_raises(ArgumentError) { HeavyHitters.new(k: 16).merge(HeavyHitters.new(k: 64)) }
    end

    def test_merge_with_the_receiver_doubles_the_counts
      table = HeavyHitters.new(k: 16)
      10.times { table.add('ip-1') }

      table.merge(table)

      assert_equal 20, table.observed
      assert_equal 20, table.count('ip-1')
    end

    def test_a_key_that_is_not_utf_8_is_rejected
      assert_raises(EncodingError) { HeavyHitters.new(k: 16).add("\xff\xfe".b) }
    end
  end
end
