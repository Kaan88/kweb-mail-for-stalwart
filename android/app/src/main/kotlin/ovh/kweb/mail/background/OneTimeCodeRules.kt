package ovh.kweb.mail.background

import java.util.regex.Pattern

/**
 * Finds a verification, login or one-time code in the text a mail notification already has (the subject and the
 * cached first lines), and decides where the "Copy code" button goes. Pure functions, tested on the JVM.
 *
 * A code is only taken when a cue word ("code", "OTP", "doğrulama", "Kod", ...) is near it, so that years, phone
 * numbers, amounts, order numbers and the like are not mistaken for one. With several candidates the one nearest a
 * cue word wins. The text is never logged and nothing here touches the network.
 */
object OneTimeCodeRules {
    /** How many characters of the preview are looked at. */
    private const val MAX_PREVIEW = 1000

    /** The most characters between a cue word and a numeric code, and between a cue word and a mixed letters-and-digits code. */
    private const val NEAR_DIGITS = 60
    private const val NEAR_MIXED = 30

    /** A year (1900 to 2099) counts as a code only when a cue word is this close. */
    private const val NEAR_YEAR = 15

    /** The longest code the copy action accepts (a defensive limit for the receiver, too). */
    const val MAX_LENGTH = 16

    private const val FLAGS = Pattern.CASE_INSENSITIVE or Pattern.UNICODE_CASE

    private const val BOUND = "(?<![\\p{L}\\p{N}])"

    /** Cue words that start a word (English, Turkish, German, French, Spanish, Italian, Dutch, Russian and others). */
    private val CUES: Pattern = Pattern.compile(
        BOUND + "(?:" +
            "c[oó]d|kod|код|otp|2fa|pass ?code|passw|contrase|mot de passe|kennwort|parol|пароль|" +
            "verif|bestätig|bestatig|confirm|подтвержд|" +
            "one[- ]?time|single[- ]use|temporary|tek kullan|geçici|gecici|einmal|" +
            "security|sicherheit|s[ée]curit|seguridad|sicurezza|güvenlik|guvenlik|" +
            "log[- ]?in|sign[- ]?in|anmeld|connexion|iniciar|giriş|giris|" +
            "auth|doğrula|dogrula|şifre|sifre|" +
            "pin(?![\\p{L}])" +
            ")",
        FLAGS,
    )

    /** Cue words of scripts written without spaces: they can sit inside a word. */
    private val LOOSE_CUES: Pattern = Pattern.compile("验证码|驗證碼|验证|驗證|인증|코드|認証|確認コード|コード")

    /** 4 to 8 digits, or two groups of 3 or 4 joined by a space or a hyphen ("123 456", "123-456"). */
    private val DIGITS: Pattern = Pattern.compile(
        // Only Latin letters stick to a code: in Chinese or Japanese the code sits right against the words.
        "(?<![\\p{IsLatin}\\p{N}.,+/@])(?<!\\d[ -])(\\d{3,4}[ -]\\d{3,4}|\\d{4,8})(?![\\p{IsLatin}\\p{N}])(?![.,]\\d)(?![ -]?\\d)",
    )

    /** 5 to 8 capital letters and digits with at least one of each ("A1B2C3"). */
    private val MIXED: Pattern = Pattern.compile("(?<![\\p{L}\\p{N}])[A-Z0-9]{5,8}(?![\\p{L}\\p{N}])")

    /** A word just before a number that says it is an order, invoice, reference or the like. */
    private val LABEL_BEFORE: Pattern = Pattern.compile(
        "(?:#|number|booking|reservation|rezervasyon|order|invoice|receipt|ref(?:erence)?|tracking|ticket|sipari[sş]|fatura|takip|bestell|commande|pedido|rechnung|заказ|订单|" +
            "(?<![\\p{L}])(?:no|nr)\\.?)\\W{0,3}$",
        FLAGS,
    )

    /** What just before a number says it is (part of) a phone number or an amount. */
    private val PHONE_BEFORE: Pattern = Pattern.compile("(?:\\+|\\)\\s?|(?<![\\p{L}])(?:tel|phone|call|ara|fax)\\W{0,3})$", FLAGS)
    private val MONEY_BEFORE: Pattern = Pattern.compile("[$€£₺¥]\\s?$")

    /** What just after a number says it is an amount or a duration ("10 minutes", "5 TL", "20%"). */
    private val AFTER_UNIT: Pattern = Pattern.compile(
        "^\\s?(?:[$€£₺¥%]|usd|eur|gbp|tl|try|dollars?|euros?|min|minute|minutes|mins|sec|seconds|hours?|dk|dakika|saniye|saat|" +
            "minuten|stunden|heures|minutos|минут)(?![\\p{L}])",
        FLAGS,
    )

    /**
     * The code in [subject] and [preview], or null. A grouped code comes back without its separator ("123 456"
     * gives "123456"); a mixed one as written.
     */
    fun find(subject: String, preview: String): String? {
        val text = subject + "\n" + preview.take(MAX_PREVIEW)
        val cues = cueSpans(text)
        if (cues.isEmpty()) return null
        var best: Candidate? = null
        fun consider(candidate: Candidate) {
            val b = best
            if (b == null || candidate.better(b)) best = candidate
        }
        val digits = DIGITS.matcher(text)
        while (digits.find()) {
            val start = digits.start(1)
            val end = digits.end(1)
            val raw = digits.group(1)
            val value = raw.filter { it.isDigit() }
            val gap = gap(cues, start, end)
            if (gap > NEAR_DIGITS || notACode(text, start, end, value, gap)) continue
            consider(Candidate(value, start, gap, afterCue(cues, start)))
        }
        val mixed = MIXED.matcher(text)
        while (mixed.find()) {
            val value = mixed.group()
            if (!value.any { it.isDigit() } || !value.any { it.isLetter() }) continue
            val gap = gap(cues, mixed.start(), mixed.end())
            if (gap > NEAR_MIXED) continue
            if (LABEL_BEFORE.matcher(text.substring(maxOf(0, mixed.start() - 16), mixed.start())).find()) continue
            consider(Candidate(value, mixed.start(), gap, afterCue(cues, mixed.start())))
        }
        return best?.value
    }

    /**
     * Which buttons a notification gets. With a code, Copy code goes first: the other buttons keep the user's order,
     * and when the user's choice already fills [NotificationActionRules.MAX] the last (the least important by the
     * user's own order) makes room. Without a code the choice is untouched.
     */
    fun plan(chosen: List<MailAction>, hasCode: Boolean): ButtonPlan {
        val now = NotificationActionRules.clean(chosen)
        return if (hasCode) ButtonPlan(true, now.take(NotificationActionRules.MAX - 1)) else ButtonPlan(false, now)
    }

    /** The buttons of one notification: [copy] first, then [actions] in order. */
    data class ButtonPlan(val copy: Boolean, val actions: List<MailAction>)

    private class Candidate(val value: String, val start: Int, val gap: Int, val after: Boolean) {
        /** Nearer a cue first; at the same distance one after the cue ("code is 123456"); then the earlier one. */
        fun better(other: Candidate): Boolean = when {
            gap != other.gap -> gap < other.gap
            after != other.after -> after
            else -> start < other.start
        }
    }

    private fun cueSpans(text: String): List<IntRange> {
        val spans = ArrayList<IntRange>()
        for (pattern in listOf(CUES, LOOSE_CUES)) {
            val m = pattern.matcher(text)
            while (m.find()) spans += m.start() until m.end()
        }
        return spans
    }

    /** The characters between the candidate and the nearest cue word; 0 when they touch or overlap. */
    private fun gap(cues: List<IntRange>, start: Int, end: Int): Int = cues.minOf {
        when {
            start > it.last -> start - (it.last + 1)
            end <= it.first -> it.first - end
            else -> 0
        }
    }

    private fun afterCue(cues: List<IntRange>, start: Int): Boolean = cues.any { start > it.last }

    /** True for a number that is a year, a phone number, an amount, a duration or an order number rather than a code. */
    private fun notACode(text: String, start: Int, end: Int, value: String, gap: Int): Boolean {
        if (value.length == 4 && value.toInt() in 1900..2099 && gap > NEAR_YEAR) return true
        val before = text.substring(maxOf(0, start - 16), start)
        val after = text.substring(end, minOf(text.length, end + 14))
        return LABEL_BEFORE.matcher(before).find() ||
            PHONE_BEFORE.matcher(before).find() ||
            MONEY_BEFORE.matcher(before).find() ||
            AFTER_UNIT.matcher(after).find()
    }
}
