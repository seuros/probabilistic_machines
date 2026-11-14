# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class HeavyHittersTest < Minitest::Test
    def test_exactly_the_supported_counter_counts_are_monomorphised
      assert_equal [16, 64, 256], HeavyHitters::COUNTERS
      HeavyHitters::COUNTERS.each do |counters|
        assert_equal counters, HeavyHitters.new(counters).counters
      end

      [1, 32, 128, 1024].each do |counters|
        assert_raises(RangeError) { HeavyHitters.new(counters) }
      end
    end

    def test_the_dominant_key_is_tracked_with_its_own_string
      table = HeavyHitters.new(16)
      5_000.times { |id| assert_equal :added, table.add("ip-#{id}") }
      1_000.times { assert_equal :added, table.add_count('ip-hot', 2) }

      assert_equal 7_000, table.observed
      assert_equal 16, table.size

      top = table.top

      assert_instance_of Hitter, top
      assert_predicate top, :frozen?
      assert_equal 'ip-hot', top.key
      assert_operator top.count, :>=, 2_000
      assert_operator top.count - top.overestimate, :<=, 2_000
    end

    def test_an_empty_table_has_no_top_and_an_untracked_key_counts_zero
      table = HeavyHitters.new(16)

      assert_nil table.top
      assert_empty table.ranked
      assert_empty table.hitters
      assert_equal 0, table.count('ip-1')
    end

    def test_ranked_is_ordered_and_hitters_hold_the_same_keys
      table = HeavyHitters.new(64)
      %w[a b c].each_with_index { |key, index| table.add_count(key, 10 - index) }

      assert_equal %w[a b c], table.ranked.map(&:key)
      assert_equal %w[a b c], table.hitters.map(&:key).sort
      assert_equal Hitter.new('a', 10, 0), table.top
    end

    def test_names_survive_eviction_churn
      table = HeavyHitters.new(16)
      5_000.times { |id| table.add("ip-#{id}") }

      assert_equal 16, table.ranked.size
      table.ranked.each { |hitter| assert_match(/\Aip-\d+\z/, hitter.key) }
    end

    def test_a_zero_count_is_refused_and_evicts_nothing
      table = HeavyHitters.new(16)
      16.times { |id| table.add("ip-#{id}") }
      before = table.encoded

      assert_equal :zero_count, table.add_count('ip-new', 0)
      assert_equal before, table.encoded
      assert_equal 0, table.count('ip-new')
    end

    def test_an_overflowing_count_is_refused_and_changes_nothing
      table = HeavyHitters.new(16)
      table.add_count('ip-1', (2**64) - 1)
      before = table.encoded

      assert_equal :overflow, table.add('ip-1')
      assert_equal :overflow, table.add('ip-2')
      assert_equal before, table.encoded
      assert_raises(RangeError) { table.add_count('ip-1', 2**64) }
    end

    def test_merge_folds_both_tables
      left = HeavyHitters.new(16)
      right = HeavyHitters.new(16)
      left.add_count('ip-1', 100)
      right.add_count('ip-2', 50)

      assert_equal :merged, left.merge(right)
      assert_equal 150, left.observed
      assert_equal %w[ip-1 ip-2], left.ranked.map(&:key)
    end

    def test_merge_refusals_change_nothing
      table = HeavyHitters.new(16)
      table.add_count('ip-1', (2**64) - 1)
      before = table.encoded
      other = HeavyHitters.new(16)
      other.add('ip-1')

      assert_equal :overflow, table.merge(other)
      assert_equal :wrong_shape, table.merge(HeavyHitters.new(64))
      assert_equal before, table.encoded
    end

    def test_merge_with_the_receiver_doubles_the_counts
      table = HeavyHitters.new(16)
      table.add_count('ip-1', 10)

      assert_equal :merged, table.merge(table)
      assert_equal 20, table.observed
      assert_equal 20, table.count('ip-1')
    end

    def test_encoded_decode_round_trips_with_the_original_keys
      table = HeavyHitters.new(16)
      5_000.times { |id| table.add("ip-#{id}") }
      1_000.times { table.add_count('ip-hot', 2) }
      blob = table.encoded

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 13 + (16 * 24), HeavyHitters.new(16).encoded.bytesize

      restored = HeavyHitters.decode(blob)

      assert_equal [16, table.observed, table.size], [restored.counters, restored.observed, restored.size]
      assert_equal table.ranked, restored.ranked
    end

    def test_decode_answers_refusal_symbols
      assert_equal :truncated, HeavyHitters.decode('')
      assert_equal :wrong_structure, HeavyHitters.decode("\xa1".b * 800)
      assert_equal :wrong_shape, HeavyHitters.decode("\xb1\x20\x00".b + ("\x00".b * 800))

      table = HeavyHitters.new(16)
      table.add('ip-1')
      frame = table.encoded

      assert_equal :malformed, HeavyHitters.decode(frame.byteslice(0, 13 + (16 * 24))), 'a key went missing'
      assert_equal :malformed, HeavyHitters.decode("#{frame}\x01".b), 'a record cut short'
    end

    def test_decode_into_replaces_table_and_names_or_changes_nothing
      table = HeavyHitters.new(16)
      table.add('mine')
      source = HeavyHitters.new(16)
      source.add_count('theirs', 3)
      before = table.encoded

      assert_equal :wrong_shape, table.decode_into(HeavyHitters.new(64).encoded)
      assert_equal :malformed, table.decode_into(source.encoded.byteslice(0, 13 + (16 * 24)))
      assert_equal before, table.encoded

      assert_equal :decoded, table.decode_into(source.encoded)
      assert_equal [Hitter.new('theirs', 3, 0)], table.ranked
    end

    # A key that is not UTF-8 text is a value out of range: RangeError, from
    # `add`, `add_count`, `count` and `Hitter.new` alike. A UTF-8 String with
    # valid bytes is text, and so are ASCII bytes in any ASCII-compatible
    # encoding; non-ASCII bytes tagged otherwise are never transcoded, even
    # when they happen to be valid UTF-8.
    def test_a_key_that_is_not_utf_8_is_rejected
      table = HeavyHitters.new(16)
      ["\xff\xfe".b, "caf\xC3\xA9".b, "caf\xE9".dup.force_encoding('ISO-8859-1'), 'ab'.encode('UTF-16LE'),
       "\xff".dup.force_encoding('UTF-8')].each do |bad|
        [-> { table.add(bad) }, -> { table.add_count(bad, 2) }, -> { table.count(bad) },
         -> { Hitter.new(bad, 2, 1) }].each do |call|
          assert_equal 'key must be valid UTF-8', assert_raises(RangeError, &call).message
        end
      end
      assert_equal 0, table.observed
      assert_equal :added, table.add('abc'.b)
      assert_equal :added, table.add('café')
      assert_equal [1, 1], [table.count('abc'), table.count('café')]
    end

    # A key is exactly a String: a Symbol, a String subclass instance and an
    # object answering to_str are TypeError, and the bytes are read natively,
    # so a redefined #bytes or #to_s on the key changes nothing.
    def test_a_key_is_exactly_a_string
      table = HeavyHitters.new(16)
      to_str = Object.new.tap { |object| def object.to_str = 'k' }
      [:k, Class.new(String).new('k'), to_str, nil].each do |bad|
        [-> { table.add(bad) }, -> { table.add_count(bad, 2) }, -> { table.count(bad) },
         -> { Hitter.new(bad, 2, 1) }].each do |call|
          assert_equal 'key must be a String', assert_raises(TypeError, &call).message
        end
      end
      assert_equal 0, table.observed
      liar = +'k'
      def liar.bytes = 'other'.bytes
      def liar.to_s = 'other'
      def liar.to_str = 'other'
      assert_equal :added, table.add(liar)
      assert_equal [1, 0], [table.count('k'), table.count('other')]
      assert_equal 'k', Hitter.new(liar, 2, 1).key
    end
  end
end
